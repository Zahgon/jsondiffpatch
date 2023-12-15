// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! JavaScript `Date` arithmetic: `Date.UTC`, `Date.prototype.toISOString` and `Date.prototype.toString` (in UTC).

const MS_PER_DAY: f64 = 86_400_000.0;
const MAX_TIME: f64 = 8.64e15;

/// `Date.UTC(year, month, day, hours, minutes, seconds, ms)` with every argument given: the time value, or `NaN`
/// when any argument is not finite or the result is out of range. Years 0 to 99 mean 1900 to 1999.
pub fn utc(year: f64, month: f64, day: f64, hours: f64, minutes: f64, seconds: f64, ms: f64) -> f64 {
    let mut year = year;
    if !year.is_nan() {
        let integer = year.trunc();
        if (0.0..=99.0).contains(&integer) {
            year = 1900.0 + integer;
        }
    }
    let day = make_day(year, month, day);
    let time = make_time(hours, minutes, seconds, ms);
    time_clip(make_date(day, time))
}

fn to_integer(value: f64) -> f64 {
    value.trunc() + 0.0
}

fn make_time(hour: f64, min: f64, sec: f64, ms: f64) -> f64 {
    if !(hour.is_finite() && min.is_finite() && sec.is_finite() && ms.is_finite()) {
        return f64::NAN;
    }
    to_integer(hour) * 3_600_000.0 + to_integer(min) * 60_000.0 + to_integer(sec) * 1000.0 + to_integer(ms)
}

fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if !(year.is_finite() && month.is_finite() && date.is_finite()) {
        return f64::NAN;
    }
    let (y, m, dt) = (to_integer(year), to_integer(month), to_integer(date));
    let ym = y + (m / 12.0).floor();
    if ym.abs() > 400_000.0 {
        return f64::NAN;
    }
    let mn = m.rem_euclid(12.0);
    let days = days_from_civil(ym as i64, mn as i64 + 1, 1);
    days as f64 + dt - 1.0
}

fn make_date(day: f64, time: f64) -> f64 {
    if !(day.is_finite() && time.is_finite()) {
        return f64::NAN;
    }
    let tv = day * MS_PER_DAY + time;
    if !tv.is_finite() {
        return f64::NAN;
    }
    tv
}

fn time_clip(time: f64) -> f64 {
    if !time.is_finite() || time.abs() > MAX_TIME {
        return f64::NAN;
    }
    to_integer(time)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The proleptic Gregorian date of a day number.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

struct Fields {
    year: i64,
    month: i64,
    day: i64,
    weekday: i64,
    hour: i64,
    minute: i64,
    second: i64,
    ms: i64,
}

fn fields(time: f64) -> Fields {
    let t = time as i64;
    let days = t.div_euclid(86_400_000);
    let in_day = t.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    Fields {
        year,
        month,
        day,
        weekday: (days + 4).rem_euclid(7),
        hour: in_day / 3_600_000,
        minute: in_day / 60_000 % 60,
        second: in_day / 1000 % 60,
        ms: in_day % 1000,
    }
}

/// `Date.prototype.toISOString` of a valid time value: `YYYY-MM-DDTHH:mm:ss.sssZ`, with a signed six-digit year
/// outside 0000 to 9999.
pub fn to_iso_string(time: f64) -> String {
    let f = fields(time);
    let year = if (0..=9999).contains(&f.year) {
        format!("{:04}", f.year)
    } else if f.year < 0 {
        format!("-{:06}", -f.year)
    } else {
        format!("+{:06}", f.year)
    };
    format!(
        "{year}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        f.month, f.day, f.hour, f.minute, f.second, f.ms
    )
}

/// `Date.prototype.toString` in the UTC time zone, or `Invalid Date`.
pub fn to_date_string(time: f64) -> String {
    if time.is_nan() {
        return "Invalid Date".to_string();
    }
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let f = fields(time);
    let year = if f.year < 0 {
        format!("-{:06}", -f.year)
    } else {
        format!("{:04}", f.year)
    };
    format!(
        "{} {} {:02} {year} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        DAYS[f.weekday as usize],
        MONTHS[(f.month - 1) as usize],
        f.day,
        f.hour,
        f.minute,
        f.second
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_matches_javascript() {
        assert_eq!(utc(2020.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0), 1_577_836_800_000.0);
        assert_eq!(
            to_iso_string(utc(50.0, 12.0, 1.0, 0.0, 0.0, 0.0, 5.0)),
            "1951-01-01T00:00:00.005Z"
        );
        assert_eq!(
            to_iso_string(utc(2020.0, 1.0, 30.0, 25.0, 61.0, 61.0, 1234.0)),
            "2020-03-02T02:02:02.234Z"
        );
        assert!(utc(275_761.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0).is_nan());
        assert_eq!(
            to_iso_string(utc(-1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0)),
            "-000001-01-01T00:00:00.000Z"
        );
        assert_eq!(
            to_iso_string(utc(10000.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0)),
            "+010000-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn date_strings() {
        assert_eq!(
            to_date_string(0.0),
            "Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)"
        );
    }
}
