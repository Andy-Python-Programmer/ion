use uefi::prelude::*;
use uefi::proto::console::text::Input;
use uefi::table::boot::{EventType, TimerTrigger, Tpl};

use crate::logger;
use crate::prelude::*;

pub fn pit_sleep_and_quit_on_keypress(system_table: &SystemTable<Boot>, seconds: u64) {
    unsafe {
        let event = system_table
            .boot_services()
            .create_event(EventType::TIMER, Tpl::CALLBACK, None)
            .expect_success("Failed to create timer event");

        let key = system_table
            .boot_services()
            .locate_protocol::<Input>()
            .expect_success("Failed to locate input protocol");

        let key = &mut *key.get();
        let wait_for_key_event = key.wait_for_key_event();

        system_table
            .boot_services()
            .set_timer(event, TimerTrigger::Relative(10000000 * seconds))
            .expect_success("Failed to create timer from event");

        loop {
            let result = system_table
                .boot_services()
                .wait_for_event(&mut [event, wait_for_key_event])
                .expect_success("Failed add event in wait queue");

            if result == 0 {
                return;
            }

            if key
                .read_key()
                .expect_success("Failed to read key")
                .is_some()
            {
                return;
            }
        }
    }
}

/// This function is responsible for intializing the boot menu.
pub fn init(system_table: &SystemTable<Boot>) {
    let mut countdown = 5;

    goto::gpoint! {'refresh:
        logger::clear();

        println!("Ion {} ", env!("CARGO_PKG_VERSION"));
        println!("Select entry:\n");

        println!(
            "Booting automatically in {}, press any key to stop the countdown...",
            countdown
        );

        countdown -= 1;

        if countdown != 0 {
            pit_sleep_and_quit_on_keypress(system_table, 1);
            continue 'refresh;
        }

        break 'refresh;
    }
}
