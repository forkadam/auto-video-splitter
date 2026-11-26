use clap::Args;
use serde_json::json;
use vsplit_core::config::load_app_config;
use vsplit_core::doctor::run_doctor;

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct DoctorArgs {}

pub fn execute(global: &GlobalOpts, _args: DoctorArgs) -> Result<i32, CliError> {
    let app_cfg = load_app_config().unwrap_or_default();
    let report = run_doctor(Some(&global.working_dir), &app_cfg);

    if report.ok {
        emit_ok(global.json, &report, || print_human(&report));
        Ok(exit_codes::OK)
    } else {
        if !global.json {
            print_human(&report);
        }
        Err(CliError::new(
            exit_codes::MISSING_DEPENDENCY,
            "MISSING_DEPENDENCY",
            "one or more required media dependencies are missing or unavailable",
        )
        .with_details(serde_json::to_value(&report).unwrap_or(json!({}))))
    }
}

fn print_human(report: &vsplit_core::doctor::DoctorReport) {
    println!(
        "vsplit {} ({}/{})",
        report.cli_shim.version, report.platform.os, report.platform.arch
    );
    println!(
        "  ffmpeg:    {} ({})",
        if report.ffmpeg.ok { "OK" } else { "MISSING" },
        report.ffmpeg.version.as_deref().unwrap_or("n/a")
    );
    println!(
        "  ffprobe:   {} ({})",
        if report.ffprobe.ok { "OK" } else { "MISSING" },
        report.ffprobe.version.as_deref().unwrap_or("n/a")
    );
    println!(
        "  encoders:  libx264={} aac={}",
        report.encoders.libx264, report.encoders.aac
    );
    if report.gstreamer.applicable {
        println!(
            "  gstreamer: avdec_h264={} fuse2={}",
            report.gstreamer.avdec_h264, report.gstreamer.fuse2
        );
        if let Some(ref hint) = report.gstreamer.hint {
            println!("  note:      {hint} (desktop video preview only)");
        }
    }
    if let Some(ref w) = report.path_shadowing.warning {
        println!("  warning:   {w}");
    }
    println!("  locks:     {} active", report.locks.len());
}
