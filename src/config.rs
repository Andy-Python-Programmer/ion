use uefi::prelude::*;
use uefi::proto::console::text::{Input, Key};
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode, RegularFile};
use uefi::table::boot::{AllocateType, MemoryType};

use crate::prelude::*;

const CONFIG_PATHS: &[&str] = &["boot\\ion.cfg", "ion.cfg"];

#[derive(Debug, Clone, Copy)]
pub enum BootProtocol {
    Stivale2,
    Stivale,
    Multiboot,
    Multiboot2,
    Linux,
}

pub struct ConfigurationEntry {
    protocol: BootProtocol,
    path: &'static str,
    name: &'static str,
    command_line: &'static str,
}

impl ConfigurationEntry {
    /// Returns the name of the configuration entry.
    #[inline]
    pub fn name(&self) -> &'static str {
        self.name
    }
}

#[derive(Debug)]
struct BootConfigutation {
    timeout: usize,
}

pub struct IonConfig {
    boot: BootConfigutation,
    pub entries: alloc::vec::Vec<ConfigurationEntry>,
}

impl IonConfig {
    /// Returns the timout in seconds for the boot menu.
    pub fn timeout(&self) -> usize {
        self.boot.timeout
    }
}

/// This function is responsible for wating for a keystroke event and returns the respective
/// key code for that keystroke.
pub fn get_char(system_table: &SystemTable<Boot>) -> Key {
    unsafe {
        // Retrieve the input protocol from the boot services,
        let input_protocol = system_table
            .boot_services()
            .locate_protocol::<Input>()
            .expect_success("Failed to locate input protocol");

        let key = &mut *input_protocol.get(); // Get the inner cell value
        let wait_for_key_event = key.wait_for_key_event(); // Get a reference to the wait for key event

        // Loop until there is a keyboard event
        loop {
            system_table
                .boot_services()
                .wait_for_event(&mut [wait_for_key_event])
                .expect_success("Failed add event in wait queue");

            // Try and read the next keystroke from the input device, if any.
            let scancode = key.read_key().expect_success("Failed to read key");

            if let Some(code) = scancode {
                return code;
            }
        }
    }
}

/// This function is responsible for loading and parsing the config file for Ion.
pub fn load(system_table: &SystemTable<Boot>, mut root: Directory) -> IonConfig {
    let mut configuration_file = None;

    // Go through each possible config path and initialize the configuration_file
    // variable if file exists.
    for filename in CONFIG_PATHS {
        let file_completion = root.open(filename, FileMode::Read, FileAttribute::empty());

        // Check if the file read operation completed with success.
        if let Ok(handle) = file_completion {
            configuration_file = Some(handle.expect("file read exited with warnings"));
            break; // Avoid to re-assign the file handle again.
        }
    }

    let configuration_file = if let Some(config) = configuration_file {
        config
    } else {
        println!("Configuration file not found.\n");

        println!("For information on the format of Ion config entries, consult CONFIG.md in");
        println!("the root of the Ion source repository.\n");

        println!("Press a key to enter an editor session and manually define a config entry...");
        let _ = get_char(system_table);

        // TODO: Print a friendly message that the configuration file does not exist and add a built-in
        // terminal way to create the config file on the fly.
        unreachable!()
    };

    let mut cfg_file_handle = unsafe { RegularFile::new(configuration_file) };

    let mut info_buf = [0; 0x100];
    let cfg_info = cfg_file_handle
        .get_info::<FileInfo>(&mut info_buf)
        .unwrap_success();

    let pages = cfg_info.file_size() as usize / 0x1000 + 1;
    let mem_start = system_table
        .boot_services()
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
        .unwrap_success();

    let buf = unsafe { core::slice::from_raw_parts_mut(mem_start as *mut u8, pages * 0x1000) };
    let len = cfg_file_handle.read(buf).unwrap_success();

    let buf = buf[..len].as_ref();
    let configuration_str = core::str::from_utf8(buf).expect("invalid UTF-8 in configuration file");

    let mut boot_config = BootConfigutation {
        // We set the default time out to 5 seconds.
        timeout: 5,
    };

    let mut entries = alloc::vec::Vec::new();

    // Create the menu tree.
    for line in configuration_str.split("\n") {
        let mut line_chars = line.chars();

        if let Some(':') = line_chars.nth(0) {
            // In this case we got a new entry.
            let config = ConfigurationEntry {
                // We use stivale 2 as the default boot protocol.
                protocol: BootProtocol::Stivale2,
                // We have already skipped the colon using line_chars.nth(0) above so the rest
                // of the line will be the kernel's name.
                name: line_chars.as_str(),
                // By default we will set the kernel command line to an empty string.
                command_line: "",
                // By default we will set the kernel path to an empty string.
                path: "",
            };

            entries.push(config);
        } else if let Some(current_entry) = entries.last_mut() {
            // Else in this case we are defining the local keys.
            if let Some(key_idx) = line.find("=") {
                let mut local_chars = line.chars();
                local_chars.nth(key_idx); // Skip the key

                let value = local_chars.as_str(); // Left with the value

                if line.starts_with("PROTOCOL=")
                    || line.starts_with("KERNEL_PROTOCOL=")
                    || line.starts_with("PROTO=")
                {
                    let protocol = match value {
                        "stivale2" => BootProtocol::Stivale2,
                        "stivale1" => BootProtocol::Stivale,
                        "stivale" => BootProtocol::Stivale,

                        "multiboot" => BootProtocol::Multiboot,
                        "multiboot1" => BootProtocol::Multiboot,
                        "multiboot2" => BootProtocol::Multiboot2,

                        "linux" => BootProtocol::Linux,

                        _ => panic!("Invalid boot protocol"),
                    };

                    current_entry.protocol = protocol;
                } else if line.starts_with("CMDLINE=") || line.starts_with("KERNEL_CMDLINE=") {
                    current_entry.command_line = value;
                } else if line.starts_with("PATH=") || line.starts_with("KERNEL_PATH=") {
                    current_entry.path = value;

                    // TODO: Do not just expect the user to give the correct kernel path and verify
                    // and parse the URI specified by the user. We will leave it as it is right now.
                }
            }
        } else {
            // In this case we got a global key.
            if let Some(key_idx) = line.find("=") {
                let mut local_chars = line.chars();
                local_chars.nth(key_idx); // Skip the key

                let value = local_chars.as_str(); // Left with the value

                if line.starts_with("TIMEOUT") {
                    let timeout =
                        value
                            .parse::<usize>()
                            .unwrap_or_else(|_| if value.eq("no") { 0 } else { 5 });

                    boot_config.timeout = timeout;
                }
            }
        }
    }

    cfg_file_handle.close();

    IonConfig {
        boot: boot_config,
        entries,
    }
}
