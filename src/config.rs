use uefi::prelude::*;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode, RegularFile};
use uefi::table::boot::{AllocateType, MemoryType};

const CONFIG_PATHS: &[&str] = &[
    "boot\\ion.cfg",
    "ion.cfg",
    // Fallback on limine configuration as they are still
    // compatable.
    "boot\\limine.cfg",
    "limine.cfg",
    // Fallback on tomato configuration as they are still
    // compatable.
    "boot\\tomatoboot.cfg",
    "tomatoboot.cgh",
];

#[derive(Debug, Clone, Copy)]
pub enum BootProtocol {
    Stivale2,
    Stivale,
    Multiboot,
    Multiboot2,
    Linux,
}

#[derive(Debug, Clone, Copy)]
struct ConfigurationEntry {
    protocol: BootProtocol,
    path: &'static str,
    name: &'static str,
    command_line: &'static str,
}

/// This function is responsible for loading and parsing the config file for Ion.
pub fn load(system_table: &SystemTable<Boot>, mut root: Directory) {
    let mut configuration_file = None;

    // Go through each possible config path and initialize the configuration_file
    // variable if file exists.
    for filename in CONFIG_PATHS {
        let file_completion = root.open(filename, FileMode::Read, FileAttribute::empty());

        // Check if the file read operation completed with success.
        if let Ok(handle) = file_completion {
            configuration_file = Some(handle.expect("File read exited with warnings"));
            break; // Avoid to re-assign the file handle again.
        }
    }

    // TODO: Print a friendly message that the configuration file does not exist and add a built-in
    // terminal way to create the config file on the fly.
    let configuration_file = configuration_file.expect("No configuration file found");
    let mut cfg_file_handle = unsafe { RegularFile::new(configuration_file) };

    let mut info_buf = [0; 0x100];
    let cfg_info = cfg_file_handle
        .get_info::<FileInfo>(&mut info_buf)
        .expect_success("Failed to get configuration file information");

    let pages = cfg_info.file_size() as usize / 0x1000 + 1;
    let mem_start = system_table
        .boot_services()
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
        .expect_success("Failed to allocate memory to read the configuration file");

    let buf = unsafe { core::slice::from_raw_parts_mut(mem_start as *mut u8, pages * 0x1000) };
    let len = cfg_file_handle
        .read(buf)
        .expect_success("Failed to read file");

    let buf = buf[..len].as_ref();
    let configuration_str = core::str::from_utf8(buf).expect("Invalid UTF-8 in configuration file");

    let mut current_entry = None;

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

            current_entry = Some(config);
        } else if let Some(mut current_entry) = current_entry {
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
        }
    }
}
