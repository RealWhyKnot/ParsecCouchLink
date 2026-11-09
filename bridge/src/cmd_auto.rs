use std::time::Duration;

use anyhow::{Result, bail};

use crate::modes::{AfterSwitch, PicoScope};
use crate::protocol::{Persona, UsbDiag};
use crate::{cmd_persona, cmd_run, cmd_usb_diag, pico_mode, support};

pub(crate) const USB_SETTLE: Duration = Duration::from_secs(5);
pub(crate) const USB_PROBE: Duration = Duration::from_secs(5);
pub(crate) const XBOX_FAMILY: &[Persona] = &[Persona::Xinput, Persona::XboxOne];
pub(crate) const PLAYSTATION_FAMILY: &[Persona] =
    &[Persona::Ps3, Persona::GenericHid, Persona::Ps4];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AutoScore {
    NoUsbTraffic = 0,
    EnumerationStarted = 1,
    ConfiguredThenUnmounted = 2,
    Suspended = 3,
    Configured = 4,
    Polling = 5,
    PollingWithOut = 6,
}

impl AutoScore {
    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::NoUsbTraffic => 0,
            Self::EnumerationStarted => 1,
            Self::ConfiguredThenUnmounted => 2,
            Self::Suspended => 3,
            Self::Configured => 4,
            Self::Polling => 5,
            Self::PollingWithOut => 6,
        }
    }
}

#[derive(Clone, Debug)]
struct AutoAttempt {
    persona: Persona,
    target: cmd_run::PicoTarget,
    score: AutoScore,
}

#[derive(Clone, Debug)]
struct AutoResult {
    target: cmd_run::PicoTarget,
    success: bool,
    score: AutoScore,
}

pub(crate) async fn run(
    selectors: Vec<String>,
    scope: PicoScope,
    after: AfterSwitch,
) -> Result<()> {
    run_with_candidates(selectors, scope, after, None).await
}

pub(crate) async fn run_family(
    selectors: Vec<String>,
    scope: PicoScope,
    after: AfterSwitch,
    candidates: &'static [Persona],
    family_label: &'static str,
) -> Result<()> {
    outln!("{family_label}: cycling {}", labels(candidates).join(", "));
    run_with_candidates(selectors, scope, after, Some(candidates)).await
}

async fn run_with_candidates(
    selectors: Vec<String>,
    scope: PicoScope,
    after: AfterSwitch,
    candidates: Option<&'static [Persona]>,
) -> Result<()> {
    let picos = cmd_run::discover_picos_with_auto_recovery(cmd_persona::DISCOVER, false).await?;
    if picos.is_empty() {
        bail!(
            "{}",
            support::no_pico_wifi_help(cmd_persona::DISCOVER.as_secs())
        );
    }

    let targets = cmd_persona::select_targets(&picos, &selectors, scope)?;
    let results = select_targets(targets, candidates).await?;

    outln!();
    outln!("Auto mode results:");
    for result in &results {
        let mark = if result.success { "ok" } else { "best" };
        outln!(
            "  [{mark}] {} selected {} ({})",
            result.target.short_label(),
            result.target.persona.label(),
            score_label(result.score)
        );
    }

    if !after.streams() {
        outln!("Run `couchlink run` to start streaming.");
        return Ok(());
    }

    let ready: Vec<_> = results
        .into_iter()
        .filter(|r| r.success)
        .map(|r| r.target)
        .collect();
    if ready.is_empty() {
        bail!("auto mode did not confirm a working gamepad persona; not starting streaming");
    }
    let routes = cmd_run::auto_routes(ready, Some((0..4).collect()))?;
    cmd_run::stream_routes(routes, cmd_run::StreamOptions::default()).await
}

pub(crate) async fn select_gamepad_targets(
    targets: Vec<cmd_run::PicoTarget>,
) -> Result<Vec<cmd_run::PicoTarget>> {
    let results = select_targets(targets, None).await?;
    let ready: Vec<_> = results
        .iter()
        .filter(|r| r.success)
        .map(|r| r.target.clone())
        .collect();
    if ready.is_empty() {
        bail!("auto mode did not confirm a working gamepad persona");
    }
    Ok(ready)
}

pub(crate) async fn select_gamepad_targets_from_candidates(
    targets: Vec<cmd_run::PicoTarget>,
    candidates: &'static [Persona],
) -> Result<Vec<cmd_run::PicoTarget>> {
    let results = select_targets(targets, Some(candidates)).await?;
    let ready: Vec<_> = results
        .iter()
        .filter(|r| r.success)
        .map(|r| r.target.clone())
        .collect();
    if ready.is_empty() {
        bail!("auto mode did not confirm a working gamepad persona");
    }
    Ok(ready)
}

async fn select_targets(
    targets: Vec<cmd_run::PicoTarget>,
    candidates: Option<&'static [Persona]>,
) -> Result<Vec<AutoResult>> {
    let mut results = Vec::new();
    for target in targets {
        results.push(select_for_target(target, candidates).await?);
    }
    Ok(results)
}

async fn select_for_target(
    target: cmd_run::PicoTarget,
    candidates: Option<&'static [Persona]>,
) -> Result<AutoResult> {
    let uid = target.info.unique_id_short;
    let mut current = target;
    let candidates = match candidates {
        Some(candidates) => family_candidates(current.persona, candidates),
        None => auto_candidates(current.persona),
    };
    let names: Vec<&str> = candidates.iter().map(|p| p.label()).collect();
    outln!();
    outln!(
        "Auto mode for {}: trying {}",
        current.short_label(),
        names.join(", ")
    );
    outln!(
        "  Keep the console adapter connected and powered; Auto scores USB enumeration and polling from that adapter."
    );

    let mut best: Option<AutoAttempt> = None;
    for candidate in candidates {
        outln!("  Trying {}...", candidate.label());
        let Some(active) = switch_to_candidate(current.clone(), candidate).await? else {
            continue;
        };
        current = active.clone();
        tokio::time::sleep(USB_SETTLE).await;

        let diag = match cmd_usb_diag::query_usb_diag(&active, USB_PROBE).await {
            Ok(diag) => diag,
            Err(e) => {
                outln!("    USB diagnostic did not reply: {e:#}");
                continue;
            }
        };
        let score = score_usb_diag(&diag);
        outln!("    {}", score_label(score));
        let attempt = AutoAttempt {
            persona: candidate,
            target: active.clone(),
            score,
        };
        if is_success_score(score) {
            outln!("    Selected {}.", candidate.label());
            return Ok(AutoResult {
                target: active,
                success: true,
                score,
            });
        }
        if best.as_ref().is_none_or(|existing| score > existing.score) {
            best = Some(attempt);
        }
    }

    let Some(best) = best else {
        bail!("{uid:08X} did not produce USB diagnostic data for any gamepad persona");
    };
    if best.target.persona != best.persona {
        outln!(
            "  Leaving {} in {} mode.",
            best.target.short_label(),
            best.target.persona.label()
        );
    }
    Ok(AutoResult {
        target: best.target,
        success: false,
        score: best.score,
    })
}

async fn switch_to_candidate(
    current: cmd_run::PicoTarget,
    candidate: Persona,
) -> Result<Option<cmd_run::PicoTarget>> {
    if current.persona == candidate {
        return Ok(Some(current));
    }

    pico_mode::request_set_persona(&current, candidate).await?;
    outln!(
        "    Waiting up to {}s for {} mode...",
        cmd_persona::REBOOT_WAIT.as_secs(),
        candidate.label()
    );
    let reappeared = cmd_persona::wait_for_persona(
        &[current.info.unique_id_short],
        candidate,
        cmd_persona::REBOOT_WAIT,
    )
    .await?;
    let found = reappeared
        .into_iter()
        .find(|p| p.info.unique_id_short == current.info.unique_id_short);
    match found {
        Some(pico) if pico.persona == candidate => Ok(Some(pico)),
        Some(pico) => {
            outln!(
                "    {} rejoined in {} mode, not {}.",
                pico.short_label(),
                pico.persona.label(),
                candidate.label()
            );
            Ok(None)
        }
        None => {
            outln!("    Pico did not rejoin Wi-Fi for this attempt.");
            Ok(None)
        }
    }
}

pub(crate) fn auto_candidates(current: Persona) -> Vec<Persona> {
    let mut out = Vec::new();
    if is_gamepad_persona(current) {
        out.push(current);
    }
    for candidate in [
        Persona::Ps3,
        Persona::GenericHid,
        Persona::Ps4,
        Persona::Xinput,
        Persona::XboxOne,
        Persona::Maple,
    ] {
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

fn is_gamepad_persona(persona: Persona) -> bool {
    matches!(
        persona,
        Persona::Xinput
            | Persona::XboxOne
            | Persona::Ps3
            | Persona::Ps4
            | Persona::Maple
            | Persona::GenericHid
    )
}

pub(crate) fn family_candidates(current: Persona, candidates: &[Persona]) -> Vec<Persona> {
    let mut out = Vec::new();
    if candidates.contains(&current) {
        out.push(current);
    }
    for candidate in candidates {
        if !out.contains(candidate) {
            out.push(*candidate);
        }
    }
    out
}

fn labels(personas: &[Persona]) -> Vec<&'static str> {
    personas.iter().map(|p| p.label()).collect()
}

pub(crate) fn score_usb_diag(diag: &UsbDiag) -> AutoScore {
    if diag.mounted() && diag.xinput_out_seen() && diag.xinput_report_sent() {
        AutoScore::PollingWithOut
    } else if diag.mounted() && diag.xinput_report_sent() {
        AutoScore::Polling
    } else if diag.mounted() {
        AutoScore::Configured
    } else {
        match diag.configuration_state() {
            crate::protocol::UsbConfigurationState::NoHostTraffic => AutoScore::NoUsbTraffic,
            crate::protocol::UsbConfigurationState::EnumerationStarted => {
                AutoScore::EnumerationStarted
            }
            crate::protocol::UsbConfigurationState::ConfiguredThenUnmounted
            | crate::protocol::UsbConfigurationState::ConfiguredThenUnmountedWithoutCallback => {
                AutoScore::ConfiguredThenUnmounted
            }
            crate::protocol::UsbConfigurationState::Suspended => AutoScore::Suspended,
            crate::protocol::UsbConfigurationState::Configured => AutoScore::Configured,
        }
    }
}

pub(crate) fn is_success_score(score: AutoScore) -> bool {
    score >= AutoScore::Polling
}

pub(crate) fn adapter_accepts_score(score: AutoScore) -> bool {
    score >= AutoScore::Configured
}

pub(crate) fn score_label(score: AutoScore) -> &'static str {
    match score {
        AutoScore::NoUsbTraffic => "no USB host enumeration traffic",
        AutoScore::EnumerationStarted => "USB host started enumeration but did not configure",
        AutoScore::ConfiguredThenUnmounted => "USB configured once, then was not mounted",
        AutoScore::Suspended => "USB suspended",
        AutoScore::Configured => "USB configured but no input report accepted yet",
        AutoScore::Polling => "USB host accepted input reports",
        AutoScore::PollingWithOut => "USB host accepted input reports and sent OUT traffic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_diag::{DiagStage, usb_diag};

    #[test]
    fn candidates_try_current_gamepad_first_without_keyboard() {
        assert_eq!(
            auto_candidates(Persona::Ps3),
            vec![
                Persona::Ps3,
                Persona::GenericHid,
                Persona::Ps4,
                Persona::Xinput,
                Persona::XboxOne,
                Persona::Maple
            ]
        );
        assert_eq!(
            auto_candidates(Persona::Keyboard),
            vec![
                Persona::Ps3,
                Persona::GenericHid,
                Persona::Ps4,
                Persona::Xinput,
                Persona::XboxOne,
                Persona::Maple
            ]
        );
        assert_eq!(
            auto_candidates(Persona::Debug),
            vec![
                Persona::Ps3,
                Persona::GenericHid,
                Persona::Ps4,
                Persona::Xinput,
                Persona::XboxOne,
                Persona::Maple
            ]
        );
    }

    #[test]
    fn family_candidates_try_current_family_member_first() {
        assert_eq!(
            family_candidates(Persona::XboxOne, &[Persona::Xinput, Persona::XboxOne]),
            vec![Persona::XboxOne, Persona::Xinput]
        );
        assert_eq!(
            family_candidates(
                Persona::Maple,
                &[Persona::Ps3, Persona::GenericHid, Persona::Ps4]
            ),
            vec![Persona::Ps3, Persona::GenericHid, Persona::Ps4]
        );
    }

    #[test]
    fn usb_diag_scoring_orders_adapter_progress() {
        assert_eq!(
            score_usb_diag(&usb_diag(DiagStage::Silent)),
            AutoScore::NoUsbTraffic
        );
        assert_eq!(
            score_usb_diag(&usb_diag(DiagStage::DescriptorsOnly)),
            AutoScore::EnumerationStarted
        );
        let mut unmounted = usb_diag(DiagStage::DescriptorsOnly);
        unmounted.mount_count = 1;
        assert_eq!(
            score_usb_diag(&unmounted),
            AutoScore::ConfiguredThenUnmounted
        );
        assert_eq!(
            score_usb_diag(&usb_diag(DiagStage::Mounted)),
            AutoScore::Configured
        );
        assert_eq!(
            score_usb_diag(&usb_diag(DiagStage::Polling)),
            AutoScore::Polling
        );
        assert_eq!(
            score_usb_diag(&usb_diag(DiagStage::PollingWithOut)),
            AutoScore::PollingWithOut
        );
    }

    #[test]
    fn success_requires_report_polling() {
        assert!(!is_success_score(AutoScore::Configured));
        assert!(is_success_score(AutoScore::Polling));
        assert!(is_success_score(AutoScore::PollingWithOut));
    }

    #[test]
    fn adapter_acceptance_includes_configured() {
        assert!(!adapter_accepts_score(AutoScore::EnumerationStarted));
        assert!(adapter_accepts_score(AutoScore::Configured));
        assert!(adapter_accepts_score(AutoScore::Polling));
    }
}
