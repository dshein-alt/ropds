use chrono::{Datelike, Local, Timelike};
use tokio::time::{Duration, sleep};
use tracing::{info, warn};

use crate::config::{Config, ScannerConfig};
use crate::db::{DbBackend, DbPool};
use crate::scanner;

/// Validate scanner schedule config values at startup.
pub fn validate_config(config: &ScannerConfig) -> Result<(), String> {
    for &m in &config.schedule_minutes {
        if m > 59 {
            return Err(format!(
                "scanner.schedule_minutes: {m} is out of range 0..=59"
            ));
        }
    }
    for &h in &config.schedule_hours {
        if h > 23 {
            return Err(format!(
                "scanner.schedule_hours: {h} is out of range 0..=23"
            ));
        }
    }
    for &d in &config.schedule_day_of_week {
        if !(1..=7).contains(&d) {
            return Err(format!(
                "scanner.schedule_day_of_week: {d} is out of range 1..=7 (Mon=1..Sun=7)"
            ));
        }
    }
    Ok(())
}

/// Check whether the current local time matches the schedule.
fn matches_schedule(config: &ScannerConfig) -> bool {
    let now = Local::now();
    let minute = now.minute();
    let hour = now.hour();
    let dow = now.weekday().number_from_monday(); // 1=Mon..7=Sun

    let minute_ok = config.schedule_minutes.is_empty() || config.schedule_minutes.contains(&minute);
    let hour_ok = config.schedule_hours.is_empty() || config.schedule_hours.contains(&hour);
    let dow_ok =
        config.schedule_day_of_week.is_empty() || config.schedule_day_of_week.contains(&dow);

    minute_ok && hour_ok && dow_ok
}

/// Format the schedule for logging.
pub fn format_schedule(config: &ScannerConfig) -> String {
    let minutes = if config.schedule_minutes.is_empty() {
        "*".to_string()
    } else {
        config
            .schedule_minutes
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let hours = if config.schedule_hours.is_empty() {
        "*".to_string()
    } else {
        config
            .schedule_hours
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let dow = if config.schedule_day_of_week.is_empty() {
        "*".to_string()
    } else {
        let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        config
            .schedule_day_of_week
            .iter()
            .map(|&d| *names.get((d - 1) as usize).unwrap_or(&"?"))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!("minutes=[{minutes}] hours=[{hours}] days=[{dow}]")
}

/// Run the scheduler loop. Checks every minute, spawns a scan task if schedule matches.
pub async fn run(pool: DbPool, config: Config, backend: DbBackend) {
    info!("Scheduler started: {}", format_schedule(&config.scanner));

    loop {
        // Sleep until the start of the next minute
        let now = Local::now();
        let secs_into_minute = now.second();
        let nanos_into_second = now.nanosecond();
        let wait = Duration::from_secs(60 - secs_into_minute as u64)
            - Duration::from_nanos(nanos_into_second as u64);
        sleep(wait).await;

        if matches_schedule(&config.scanner) {
            info!("Scheduled scan triggered");
            let pool = pool.clone();
            let config = config.clone();
            tokio::spawn(async move {
                match scanner::run_scan(&pool, &config, backend).await {
                    Ok(stats) => {
                        info!(
                            "Scheduled scan finished: added={}, skipped={}, deleted={}, archives_scanned={}, archives_skipped={}, errors={}",
                            stats.books_added,
                            stats.books_skipped,
                            stats.books_deleted,
                            stats.archives_scanned,
                            stats.archives_skipped,
                            stats.errors,
                        );
                    }
                    Err(scanner::ScanError::AlreadyRunning) => {
                        warn!("Scheduled scan skipped: scan already running");
                    }
                    Err(e) => {
                        warn!("Scheduled scan failed: {e}");
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ScannerConfig;

    fn make_config(minutes: Vec<u32>, hours: Vec<u32>, dow: Vec<u32>) -> ScannerConfig {
        ScannerConfig {
            schedule_minutes: minutes,
            schedule_hours: hours,
            schedule_day_of_week: dow,
            delete_logical: true,
            skip_unchanged: false,
            test_zip: false,
            test_files: false,
            workers_num: 1,
        }
    }

    #[test]
    fn test_validate_config_ok() {
        assert!(validate_config(&make_config(vec![0, 30], vec![12], vec![1, 7])).is_ok());
        assert!(validate_config(&make_config(vec![], vec![], vec![])).is_ok());
    }

    #[test]
    fn test_validate_config_bad_minute() {
        let err = validate_config(&make_config(vec![60], vec![], vec![]));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("60"));
    }

    #[test]
    fn test_validate_config_bad_hour() {
        let err = validate_config(&make_config(vec![], vec![24], vec![]));
        assert!(err.is_err());
    }

    #[test]
    fn test_validate_config_bad_dow() {
        assert!(validate_config(&make_config(vec![], vec![], vec![0])).is_err());
        assert!(validate_config(&make_config(vec![], vec![], vec![8])).is_err());
    }

    #[test]
    fn test_format_schedule_defaults() {
        let config = make_config(vec![0], vec![0, 12], vec![]);
        let s = format_schedule(&config);
        assert_eq!(s, "minutes=[0] hours=[0,12] days=[*]");
    }

    #[test]
    fn test_format_schedule_with_days() {
        let config = make_config(vec![30], vec![23], vec![1, 4]);
        let s = format_schedule(&config);
        assert_eq!(s, "minutes=[30] hours=[23] days=[Mon,Thu]");
    }
}
