use crate::config::ConfigurationEntry;

use crate::config;
use crate::logger;

use uefi::prelude::*;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileMode};

pub fn boot(system_table: &SystemTable<Boot>, root: &mut Directory, entry: ConfigurationEntry) {
    logger::clear();
    logger::flush();

    let parsed_uri = config::parse_uri(entry.path()).expect("stivale2: failed to parse the URI");
    let uri = config::handle_uri_redirect(&parsed_uri, root);

    assert_ne!(entry.path().len(), 0, "stivale2: KERNEL_PATH not specified");

    let kernel_path = entry.path();
    let file_completion = uri
        .open(parsed_uri.path(), FileMode::Read, FileAttribute::empty())
        .expect_success("stivale2: failed to open kernel file. Is its path correct?");

    log::debug!("stivale2: loading kernel {}...\n", kernel_path);

    logger::flush();
}
