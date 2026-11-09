use super::HOME_SCAN_SECONDS;
use crate::{cdc, cmd_flash, cmd_run};
use anyhow::Result;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub(super) struct PicoInventory {
    pub(super) wifi: Vec<cmd_run::PicoTarget>,
    pub(super) usb: Vec<SetupUsbPico>,
    pub(super) usb_errors: Vec<(String, String)>,
    pub(super) bootsel: Vec<BootselPico>,
    pub(super) scan_errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SetupUsbPico {
    pub(super) port: String,
    pub(super) unique_id_short: Option<u32>,
    pub(super) board_type: u8,
    pub(super) firmware: String,
    pub(super) fw_major: u8,
    pub(super) fw_minor: u8,
    pub(super) fw_patch: u8,
    pub(super) creds_present: bool,
    pub(super) self_test_passed: bool,
    pub(super) self_test_message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BootselPico {
    pub(super) mount: PathBuf,
    pub(super) board: cmd_flash::BootselBoard,
}

pub(super) async fn scan_pico_inventory() -> Result<PicoInventory> {
    let mut inventory = PicoInventory::default();
    match cmd_run::discover_picos(Duration::from_secs(HOME_SCAN_SECONDS)).await {
        Ok(wifi) => inventory.wifi = wifi,
        Err(e) => inventory
            .scan_errors
            .push(format!("Wi-Fi discovery failed: {e:#}")),
    }
    match scan_setup_usb_picos().await {
        Ok((usb, usb_errors)) => {
            inventory.usb = usb;
            inventory.usb_errors = usb_errors;
        }
        Err(e) => inventory
            .scan_errors
            .push(format!("setup USB scan failed: {e:#}")),
    }
    inventory.bootsel = cmd_flash::visible_bootsel_mounts()
        .into_iter()
        .map(|(mount, board)| BootselPico { mount, board })
        .collect();
    Ok(inventory)
}

pub(super) async fn scan_setup_usb_picos() -> Result<(Vec<SetupUsbPico>, Vec<(String, String)>)> {
    let ports = cdc::find_setup_ports()?;
    let mut found = Vec::new();
    let mut errors = Vec::new();
    for port in ports {
        match setup_usb_info(port.clone()).await {
            Ok(info) => found.push(info),
            Err(e) => errors.push((port, format!("{e:#}"))),
        }
    }
    Ok((found, errors))
}

pub(super) async fn setup_usb_info(port: String) -> Result<SetupUsbPico> {
    tokio::task::spawn_blocking(move || -> Result<SetupUsbPico> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        let self_test = pico.self_test()?;
        let unique_id_short = pico.unique_id_short().ok();
        Ok(SetupUsbPico {
            port,
            unique_id_short,
            board_type: hello.board_type,
            firmware: hello.firmware_version().to_string(),
            fw_major: hello.fw_major,
            fw_minor: hello.fw_minor,
            fw_patch: hello.fw_patch,
            creds_present: hello.creds_present(),
            self_test_passed: self_test.passed,
            self_test_message: self_test.message,
        })
    })
    .await?
}
