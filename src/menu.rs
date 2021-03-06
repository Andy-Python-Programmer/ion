use uefi::prelude::*;
use uefi::proto::console::text::{Input, Key, ScanCode};
use uefi::table::boot::{EventType, TimerTrigger, Tpl};

use crate::config::{self, ConfigurationEntry};
use crate::logger;

use crate::config::IonConfig;
use crate::logger::Color;

use crate::prelude::*;

/// This function is responsible for sleeping the provided amount of `seconds` and if
/// a special key is pressed in the duration specified, the function will return the keyboard
/// scancode and quit the timer. Else the function will return [`None`].
pub fn pit_sleep_and_quit_on_keypress(
    system_table: &SystemTable<Boot>,
    seconds: usize,
) -> Option<ScanCode> {
    unsafe {
        // Create a new timer event with the TPL set to callback.
        let event = system_table
            .boot_services()
            .create_event(EventType::TIMER, Tpl::CALLBACK, None)
            .expect_success("Failed to create timer event");

        // Retrieve the input protocol from the boot services,
        let input_protocol = system_table
            .boot_services()
            .locate_protocol::<Input>()
            .expect_success("Failed to locate input protocol");

        let key = &mut *input_protocol.get(); // Get the inner cell value
        let wait_for_key_event = key.wait_for_key_event(); // Get a reference to the wait for key event

        // Initialize the timer event that we created before and set the amount of seconds requested.
        system_table
            .boot_services()
            .set_timer(event, TimerTrigger::Relative(10000000 * seconds as u64))
            .expect_success("Failed to create timer from event");

        // Loop until the timer finishes or interrupted by a keyboard interrupt.
        loop {
            let result = system_table
                .boot_services()
                .wait_for_event(&mut [event, wait_for_key_event])
                .expect_success("Failed add event in wait queue");

            // If the result is equal to zero, that means our timer event has finished and return. Since
            // we did not retrieve a scancode we return [`None`].
            if result == 0 {
                return None;
            }

            // Try and read the next keystroke from the input device, if any.
            let scancode = key.read_key().expect_success("Failed to read key");

            // Check if there is any keystore.
            if let Some(code) = scancode {
                // If the key stroke is classified as special we return the keyboard scancode
                // and quit the timer.
                if let Key::Special(special) = code {
                    return Some(special);
                } else {
                    // Else if the key stroke is not classified as special, we will still quit the
                    // timer as the user might want to access the boot menu. To overcome this issue
                    // we will return a null scancode.
                    return Some(ScanCode::NULL);
                }
            }
        }
    }
}

/// Helper function used to print the boot menu tree.
fn print_tree(boot_config: &IonConfig, selected_entry: usize) {
    for (i, entry) in boot_config.entries.iter().enumerate() {
        if i == selected_entry {
            logger::with_fg(Color::new(0xFFAAF), || {
                println!("{}", entry.name());
            })
        } else {
            println!("{}", entry.name());
        }
    }

    logger::flush();
}

/// This function is responsible for intializing the boot menu. This function returns the
/// index of the selected boot entry.
pub fn init(system_table: &SystemTable<Boot>, boot_config: IonConfig) -> ConfigurationEntry {
    let mut selected_entry = 0;
    let mut done_timeout = false;

    loop {
        logger::clear();

        println!("Ion {} ", env!("CARGO_PKG_VERSION"));
        println!("Select entry:\n");

        print_tree(&boot_config, selected_entry);

        if !done_timeout {
            for i in (0..boot_config.timeout()).rev() {
                logger::set_cursor_pos(0, logger::display_height() - 24);
                logger::set_scroll_lock(true);

                println!(
                    "Booting automatically in {}, press any key to stop the countdown...",
                    i
                );

                logger::flush();
                logger::set_scroll_lock(false);

                if pit_sleep_and_quit_on_keypress(system_table, 1).is_some() {
                    break;
                }
            }

            done_timeout = true;
            continue;
        }

        loop {
            match config::get_char(system_table) {
                Key::Special(code) => match code {
                    ScanCode::UP => {
                        selected_entry = selected_entry
                            .checked_sub(1)
                            .unwrap_or(boot_config.entries.len() - 1);

                        // Breaking out of this loop will cause the parent draw loop to
                        // continue.
                        break;
                    }

                    ScanCode::DOWN => {
                        selected_entry += 1;

                        if selected_entry >= boot_config.entries.len() {
                            selected_entry = 0;
                        }

                        // Breaking out of this loop will cause the parent draw loop to
                        // continue.
                        break;
                    }

                    _ => (),
                },

                Key::Printable(c) => {
                    let c: char = c.into();

                    match c {
                        // UEFI wierdness the ENTER key returns a carriage return so we have to match
                        // on that.
                        '\r' => {
                            return boot_config.entries[selected_entry].clone();
                        }

                        _ => (),
                    }
                }
            }
        }
    }
}
