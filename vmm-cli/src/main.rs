use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand};
use tabled::{Table, Tabled};
use vmm_core::config::{validate_vm_name, VmConfig, VmConfigIo};
use vmm_core::connection::HypervisorConnection;
use vmm_core::template::builtin_templates;

// SECURITY: Validate a VM name from CLI input before passing it to any vmm-core
// function. Prevents injection via crafted names in XML, virsh commands, or file
// paths. (CWE-20: Improper Input Validation, CWE-88: Improper Neutralization of
// Argument Delimiters in a Command)
fn checked_vm_name(name: &str) -> Result<&str> {
    if let Some(err) = validate_vm_name(name) {
        bail!("Invalid VM name '{}': {}", name, err);
    }
    Ok(name)
}

// SECURITY: Validate an ISO path from CLI input. Rejects relative paths, path
// traversal sequences, and null bytes that could be used to escape path boundaries.
// (CWE-22: Path Traversal, CWE-59: Improper Link Resolution)
fn checked_iso_path(path: &str) -> Result<String> {
    if path.contains('\0') {
        bail!("ISO path contains null byte");
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        bail!("ISO path must be absolute: {}", path);
    }
    // Reject path traversal components
    for component in p.components() {
        if let std::path::Component::ParentDir = component {
            bail!("ISO path must not contain '..': {}", path);
        }
    }
    Ok(path.to_string())
}

#[derive(Parser)]
#[command(name = "vmm", version, about = "Libre VMM — Command-line VM manager")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all virtual machines
    List,

    /// Create a new virtual machine
    Create {
        /// VM name
        name: String,
        /// OS template (ubuntu-desktop, windows-11, fedora-workstation, etc.)
        #[arg(short, long, default_value = "ubuntu-desktop")]
        template: String,
        /// Path to installer ISO
        #[arg(short, long)]
        iso: Option<String>,
        /// Number of CPUs
        #[arg(long)]
        cpus: Option<u32>,
        /// Memory in MiB
        #[arg(long)]
        memory: Option<u64>,
        /// Disk size in GiB
        #[arg(long)]
        disk: Option<u64>,
    },

    /// Start a virtual machine
    Start {
        /// VM name
        name: String,
    },

    /// Shut down a virtual machine gracefully
    Shutdown {
        /// VM name
        name: String,
    },

    /// Force-stop a virtual machine
    #[command(alias = "kill")]
    Stop {
        /// VM name
        name: String,
    },

    /// Pause a running virtual machine
    Pause {
        /// VM name
        name: String,
    },

    /// Resume a paused virtual machine
    Resume {
        /// VM name
        name: String,
    },

    /// Open the graphical console
    Console {
        /// VM name
        name: String,
    },

    /// Delete a virtual machine
    Delete {
        /// VM name
        name: String,
        /// Also delete the disk image
        #[arg(long, default_value = "true")]
        delete_disk: bool,
    },

    /// List available OS templates
    Templates,

    /// Suspend a VM to disk (hibernate)
    Suspend {
        /// VM name
        name: String,
    },

    /// Reboot a virtual machine
    Reboot {
        /// VM name
        name: String,
    },

    /// Clone a virtual machine
    Clone {
        /// Source VM name
        name: String,
        /// New VM name
        new_name: String,
        /// Clone type: full or linked
        #[arg(long, default_value = "full")]
        clone_type: String,
    },

    /// Manage snapshots
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// Compact a qcow2 disk image to reclaim unused space
    Compact {
        /// VM name
        name: String,
    },

    /// Import a VM from another hypervisor format
    Import {
        /// Path to source file (.xml, .vmx, .vbox, .conf)
        path: String,
    },

    /// Show system/hypervisor information
    Info,

    /// Generate shell completion scripts.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
enum SnapshotAction {
    /// List snapshots for a VM
    List {
        /// VM name
        name: String,
    },
    /// Create a snapshot
    Create {
        /// VM name
        name: String,
        /// Snapshot name
        #[arg(short, long)]
        snap_name: String,
        /// Description
        #[arg(short, long, default_value = "")]
        description: String,
    },
    /// Revert to a snapshot
    Revert {
        /// VM name
        name: String,
        /// Snapshot name
        snap_name: String,
    },
    /// Delete a snapshot
    Delete {
        /// VM name
        name: String,
        /// Snapshot name
        snap_name: String,
    },
}

#[derive(Tabled)]
struct VmRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "State")]
    state: String,
    #[tabled(rename = "vCPUs")]
    vcpus: u32,
    #[tabled(rename = "Memory (MiB)")]
    memory: u64,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("vmm=info").init();

    let cli = Cli::parse();

    // Handle completion generation without requiring a hypervisor connection.
    if let Commands::Completions { shell } = &cli.command {
        let mut cmd = Cli::command();
        clap_complete::generate(*shell, &mut cmd, "vmm", &mut std::io::stdout());
        return Ok(());
    }

    let conn = HypervisorConnection::connect_best()?;

    match cli.command {
        Commands::List => {
            let vms = conn.list_vms()?;
            if vms.is_empty() {
                println!("No virtual machines found.");
                println!("Create one with: vmm create \"My VM\" --template ubuntu-desktop --iso /path/to/ubuntu.iso");
                return Ok(());
            }
            let rows: Vec<VmRow> = vms
                .into_iter()
                .map(|vm| VmRow {
                    name: vm.name,
                    state: vm.state.to_string(),
                    vcpus: vm.vcpus,
                    memory: vm.memory_mib,
                })
                .collect();
            println!("{}", Table::new(rows));
        },

        Commands::Create {
            name,
            template,
            iso,
            cpus,
            memory,
            disk,
        } => {
            // SECURITY: Validate VM name before use (CWE-20)
            checked_vm_name(&name)?;

            let templates = builtin_templates();
            let tmpl = templates
                .iter()
                .find(|t| t.id == template)
                .unwrap_or_else(|| {
                    eprintln!(
                        "Unknown template '{}'. Use 'vmm templates' to see available templates.",
                        template
                    );
                    std::process::exit(1);
                });

            // SECURITY: Validate ISO path before passing to config (CWE-22, CWE-59)
            let validated_iso = match iso {
                Some(ref path) => Some(checked_iso_path(path)?),
                None => None,
            };

            let mut config = VmConfig::from_template(&name, tmpl, validated_iso);
            if let Some(c) = cpus {
                config.vcpus = c;
            }
            if let Some(m) = memory {
                config.memory_mib = m;
            }
            if let Some(d) = disk {
                config.disk_size_gib = d;
            }

            conn.create_vm(&config)?;
            println!("VM '{}' created successfully.", name);
            println!("Start it with: vmm start \"{}\"", name);
        },

        Commands::Start { name } => {
            // SECURITY: Validate VM name before use (CWE-20, CWE-88)
            let name = checked_vm_name(&name)?;
            conn.start_vm(name)?;
            println!("VM '{}' started.", name);
            println!("Open console with: vmm console \"{}\"", name);
        },

        Commands::Shutdown { name } => {
            let name = checked_vm_name(&name)?;
            conn.shutdown_vm(name)?;
            println!("Shutdown signal sent to '{}'.", name);
        },

        Commands::Stop { name } => {
            let name = checked_vm_name(&name)?;
            conn.force_stop_vm(name)?;
            println!("VM '{}' force-stopped.", name);
        },

        Commands::Pause { name } => {
            let name = checked_vm_name(&name)?;
            conn.pause_vm(name)?;
            println!("VM '{}' paused.", name);
        },

        Commands::Resume { name } => {
            let name = checked_vm_name(&name)?;
            conn.resume_vm(name)?;
            println!("VM '{}' resumed.", name);
        },

        Commands::Console { name } => {
            let name = checked_vm_name(&name)?;
            conn.open_console(name)?;
            println!("Console opened for '{}'.", name);
        },

        Commands::Delete { name, delete_disk } => {
            let name = checked_vm_name(&name)?;
            conn.delete_vm(name, delete_disk)?;
            println!("VM '{}' deleted.", name);
        },

        Commands::Templates => {
            println!("Available OS templates:\n");
            for t in builtin_templates() {
                println!(
                    "  {:<20} {} CPUs | {} MiB RAM | {} GiB disk",
                    t.id, t.recommended_cpus, t.recommended_memory_mib, t.recommended_disk_gib
                );
                println!("  {:<20} {}\n", "", t.description);
            }
        },

        Commands::Suspend { name } => {
            let name = checked_vm_name(&name)?;
            conn.suspend_to_disk(name)?;
            println!("VM '{}' suspended to disk.", name);
        },

        Commands::Reboot { name } => {
            let name = checked_vm_name(&name)?;
            conn.reboot_vm(name)?;
            println!("Reboot signal sent to '{}'.", name);
        },

        Commands::Clone {
            name,
            new_name,
            clone_type,
        } => {
            let name = checked_vm_name(&name)?;
            checked_vm_name(&new_name)?;
            let ct = match clone_type.to_lowercase().as_str() {
                "linked" => vmm_core::CloneType::Linked,
                _ => vmm_core::CloneType::Full,
            };
            let configs = VmConfig::list_all()?;
            let source = configs
                .iter()
                .find(|c| c.name == name)
                .ok_or_else(|| anyhow::anyhow!("VM config '{}' not found", name))?;
            let new_config = vmm_core::clone::clone_vm(&conn, source, &new_name, &ct)?;
            println!(
                "VM '{}' cloned as '{}' ({} clone).",
                name, new_config.name, clone_type
            );
        },

        Commands::Snapshot { action } => match action {
            SnapshotAction::List { name } => {
                let name = checked_vm_name(&name)?;
                let snaps = vmm_core::snapshot::list_snapshots(conn.raw_conn(), name)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if snaps.is_empty() {
                    println!("No snapshots for '{}'.", name);
                } else {
                    println!("Snapshots for '{}':\n", name);
                    for s in &snaps {
                        println!("  {} — {} ({})", s.name, s.description, s.state);
                    }
                }
            },
            SnapshotAction::Create {
                name,
                snap_name,
                description,
            } => {
                let name = checked_vm_name(&name)?;
                vmm_core::snapshot::create_snapshot(
                    conn.raw_conn(),
                    name,
                    &snap_name,
                    &description,
                )
                .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("Snapshot '{}' created for '{}'.", snap_name, name);
            },
            SnapshotAction::Revert { name, snap_name } => {
                let name = checked_vm_name(&name)?;
                vmm_core::snapshot::revert_snapshot(conn.raw_conn(), name, &snap_name)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("Reverted '{}' to snapshot '{}'.", name, snap_name);
            },
            SnapshotAction::Delete { name, snap_name } => {
                let name = checked_vm_name(&name)?;
                vmm_core::snapshot::delete_snapshot(conn.raw_conn(), name, &snap_name)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("Snapshot '{}' deleted from '{}'.", snap_name, name);
            },
        },

        Commands::Compact { name } => {
            let name = checked_vm_name(&name)?;
            let configs = VmConfig::list_all()?;
            let config = configs
                .iter()
                .find(|c| c.name == name)
                .ok_or_else(|| anyhow::anyhow!("VM config '{}' not found", name))?;
            println!("Compacting disk for '{}'... this may take a while.", name);
            let saved = vmm_core::disk_manage::compact_disk(&config.disk_path)?;
            let saved_mb = saved / (1024 * 1024);
            println!("Done! Reclaimed {} MiB of disk space.", saved_mb);
        },

        Commands::Import { path } => {
            let p = std::path::Path::new(&path);
            if !p.exists() {
                bail!("File not found: {}", path);
            }
            let imported = vmm_core::import::parse_import(p)
                .map_err(|e| anyhow::anyhow!("Import parse error: {}", e))?;
            println!("Detected VM: {}", imported.name);
            println!("  OS Type:  {:?}", imported.os_type);
            println!("  vCPUs:    {}", imported.vcpus);
            println!("  Memory:   {} MiB", imported.memory_mib);
            println!("  Disks:    {}", imported.disk_paths.len());
            for d in &imported.disk_paths {
                println!("    - {}", d.display());
            }
            let config = vmm_core::import::to_vm_config(&imported);
            config.save()?;
            conn.create_vm(&config)?;
            println!("\nVM '{}' imported and defined successfully.", config.name);
            println!("Start it with: vmm start \"{}\"", config.name);
        },

        Commands::Info => {
            println!("Libre VMM — System Info\n");
            println!("  Hypervisor:      {}", conn.hypervisor_info()?);
            println!(
                "  KVM Available:   {}",
                if conn.kvm_available() {
                    "Yes"
                } else {
                    "No (emulation only)"
                }
            );
            println!("  Config Dir:      {}", VmConfig::config_dir());
            println!("  Disk Dir:        {}", VmConfig::default_vm_dir());
        },

        Commands::Completions { .. } => {
            // Handled above before hypervisor connection.
            unreachable!()
        },
    }

    Ok(())
}
