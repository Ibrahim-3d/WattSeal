# Energy summary dashboard and background startup

This document describes the calendar energy summaries, cost projections, tray metrics, and Windows login-start behavior implemented by the energy dashboard feature.

## Design goals

- Keep WattSeal's existing sensor collection and estimation logic as the single source of truth.
- Derive user-facing energy and cost summaries from the existing `total_data` history instead of introducing a second counter or database schema.
- Make the common case readable at a glance: current power, today, month-to-date, and a clearly identified monthly projection.
- Preserve the existing detailed component, process, and historical charts below the summary.
- Reuse the existing `--background` lifecycle mode for login startup rather than adding a second hidden-window implementation.
- Keep all data local.

## Data source and accuracy

The summary uses the same estimated total-system energy that WattSeal already stores in `total_data`. Hardware-backed sensors (for example supported CPU energy counters and NVIDIA NVML) remain measured by their existing implementations; components without direct energy telemetry remain WattSeal estimates.

These values are therefore **software estimates of computer energy use**, not utility-grade measurements at the AC wall outlet. PSU conversion losses or devices that WattSeal cannot observe may not be represented exactly.

No new sensor path, sampling loop, or persistent energy counter is added by this feature.

## Calendar aggregation

`common/src/usage.rs` exposes `Database::get_usage_summary()` and a short-lived cached helper for frequently rendered UI views.

Calendar boundaries use the machine's local timezone:

- **Today**: local midnight to now.
- **This month**: first day of the local calendar month to now.
- **Recent average**: up to the most recent seven calendar-day equivalents for which history exists.

WattSeal compacts older one-second `total_data` records into hourly records. A summary interval may therefore start or end in the middle of an hourly record. The aggregation query prorates the record by the portion of its `duration_ms` that overlaps the requested interval:

```text
overlap_ms = max(0, min(record_end, range_end) - max(record_start, range_start))
weighted_energy = record_energy * overlap_ms / record_duration_ms
```

This prevents an hourly bucket crossing midnight or a month boundary from being counted wholly on one side of that boundary.

## Metrics

### Current power

The latest live `total_data` row is converted from energy over its sampling duration to watts.

```text
current_power_W = energy_J / sample_duration_s
```

### Current cost per hour

This is a rate based on the current estimated power draw, not a historical average:

```text
cost_per_hour = current_power_W / 1000 * configured_price_per_kWh
```

### Today / month-to-date

The dashboard shows both energy and cost:

```text
cost = energy_Wh / 1000 * configured_price_per_kWh
```

### Recent daily average

The recent energy total is divided by elapsed **calendar time**, not only by WattSeal runtime. This means periods when the computer is off correctly lower the daily-use average once history spans those periods.

```text
recent_daily_average_Wh = recent_energy_Wh / recent_elapsed_days
```

The lookback is capped at seven days. On a new installation, the actual shorter history duration is used and displayed as the projection basis.

### Average active hour

This answers "how much energy does the machine consume during an average hour while WattSeal is actually monitoring it?":

```text
average_active_hour_Wh = recent_energy_Wh / monitored_hours
```

It intentionally differs from the calendar-hour average because it excludes periods when the collector was not running.

### Monitored today

The monitored duration is the sum of persisted sample durations overlapping today's local calendar interval. It is useful for interpreting a day's energy total when WattSeal was not running continuously.

### Monthly projection

The forecast does not multiply the current day by 30. It uses actual month-to-date energy plus the recent daily average for the remaining fraction of the current calendar month:

```text
projected_month_Wh = month_to_date_Wh + recent_daily_average_Wh * remaining_calendar_days
```

A new installation with only a short amount of history can produce a volatile projection. The dashboard displays how many days of history are currently informing the estimate; confidence improves as that approaches seven days.

## Carbon estimates

Today's emissions use the same user-configured grid carbon intensity as the existing all-time emissions display:

```text
CO2_g = energy_Wh / 1000 * configured_gCO2_per_kWh
```

## System tray

The root WattSeal process owns the system tray. The tray now exposes a compact readout refreshed every five seconds:

- Current estimated power and current cost/hour.
- Today's energy and cost.
- Projected monthly energy and cost.
- `Open UI` and `Quit` retain their existing behavior.

The tray tooltip also contains current power and today's energy/cost so the dashboard does not need to be opened for a quick check.

Tray menu events are processed on the native event-loop thread. This allows tray labels and check states to be updated without moving non-`Send` tray objects across threads.

## Start with Windows

On Windows, the tray contains **Start with Windows (background)**.

Enabling it creates the per-user registry value:

```text
HKCU\Software\Microsoft\Windows\CurrentVersion\Run\WattSeal
```

with a command equivalent to:

```text
"C:\path\to\WattSeal.exe" --background
```

The existing `--background` mode starts the collector and tray without spawning the dashboard window, so startup does not create a taskbar window. Double-clicking the tray icon or choosing `Open UI` opens the dashboard later.

The HKCU Run key does not require administrator privileges. Registry commands are launched without a console window. Disabling the setting removes only WattSeal's own value.

Other platforms leave this startup control disabled; no platform-specific autostart behavior is silently invented.

## Persistence and migrations

This feature adds **no database migration**. Energy summaries are derived from existing `total_data`, while Windows startup state is read directly from the operating-system registry.

The existing electricity price, currency, and carbon settings remain the source of truth for all derived costs and emissions.

## Verification

Before submitting upstream:

```bash
cargo +nightly fmt -- --check
cargo build
cargo test -p common
```

Recommended manual Windows checks:

1. Run WattSeal normally and confirm the dashboard summary updates while detailed charts still work.
2. Compare `Today` energy before and after a known load to confirm it increases monotonically.
3. Change electricity price/currency and confirm dashboard/tray cost values update.
4. Toggle `Start with Windows (background)`, sign out/in, and confirm only the tray starts.
5. Open the UI from the tray, close the UI, and confirm collection continues.
6. Disable startup and verify the WattSeal Run value is removed.
