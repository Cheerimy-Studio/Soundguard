use std::fmt::Write as _;

use chrono::{DateTime, Duration, Local, Timelike, Utc};
use tracing::{error, info};

use crate::{
    mailer::MailAttachment,
    models::{round1, Alert, AlertKind, Reading, ReportResult, Stats},
    state::AppState,
};

pub fn readings_csv(readings: &[Reading]) -> anyhow::Result<Vec<u8>> {
    let mut writer = csv::WriterBuilder::new().from_writer(vec![0xEF, 0xBB, 0xBF]);
    writer.write_record([
        "device_id",
        "recorded_at",
        "avg_db",
        "peak_db",
        "min_db",
        "over_threshold",
        "duration_secs",
    ])?;
    for reading in readings {
        writer.write_record([
            reading.device_id.as_str(),
            &reading.recorded_at.to_rfc3339(),
            &format!("{:.1}", reading.avg_db),
            &format!("{:.1}", reading.peak_db),
            &format!("{:.1}", reading.min_db),
            if reading.over_threshold { "true" } else { "false" },
            &reading.duration_secs.to_string(),
        ])?;
    }
    Ok(writer.into_inner()?)
}

pub fn downsample(readings: &[Reading], max_points: usize) -> Vec<Reading> {
    if readings.len() <= max_points || max_points < 2 {
        return readings.to_vec();
    }
    let bucket_size = readings.len().div_ceil(max_points);
    readings
        .chunks(bucket_size)
        .map(|chunk| {
            let first = &chunk[0];
            let count = chunk.len() as f64;
            Reading {
                device_id: first.device_id.clone(),
                recorded_at: first.recorded_at,
                avg_db: round1(chunk.iter().map(|r| r.avg_db).sum::<f64>() / count),
                peak_db: chunk
                    .iter()
                    .map(|r| r.peak_db)
                    .fold(f64::NEG_INFINITY, f64::max),
                min_db: chunk
                    .iter()
                    .map(|r| r.min_db)
                    .fold(f64::INFINITY, f64::min),
                over_threshold: chunk.iter().any(|r| r.over_threshold),
                duration_secs: chunk.iter().map(|r| r.duration_secs).sum(),
                sample_count: chunk.iter().map(|r| r.sample_count).sum(),
            }
        })
        .collect()
}

pub fn render_svg(
    device_name: &str,
    threshold_db: f64,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    readings: &[Reading],
    stats: &Stats,
) -> String {
    let points = downsample(readings, 720);
    let width = 1200.0;
    let height = 460.0;
    let left = 72.0;
    let right = 40.0;
    let top = 88.0;
    let bottom = 64.0;
    let plot_w = width - left - right;
    let plot_h = height - top - bottom;

    let mut min_y = 30.0_f64;
    let mut max_y = 100.0_f64;
    if let (Some(min), Some(max)) = (stats.min_db, stats.peak_db) {
        min_y = (min - 5.0).floor().max(20.0);
        max_y = (max + 5.0).ceil().min(140.0);
    }
    max_y = max_y.max(threshold_db + 5.0);
    if max_y - min_y < 20.0 {
        max_y = min_y + 20.0;
    }
    let y_for = |db: f64| -> f64 {
        let ratio = ((db - min_y) / (max_y - min_y)).clamp(0.0, 1.0);
        top + plot_h * (1.0 - ratio)
    };
    let x_for = |time: DateTime<Utc>| -> f64 {
        let total = (to - from).num_milliseconds().max(1) as f64;
        let elapsed = (time - from).num_milliseconds().clamp(0, total as i64) as f64;
        left + plot_w * (elapsed / total)
    };

    let mut svg = String::new();
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<defs>
  <linearGradient id="area" x1="0" x2="0" y1="0" y2="1">
    <stop offset="0%" stop-color="#0ea5b7" stop-opacity="0.34"/>
    <stop offset="100%" stop-color="#0ea5b7" stop-opacity="0.02"/>
  </linearGradient>
  <linearGradient id="line" x1="0" x2="1" y1="0" y2="0">
    <stop offset="0%" stop-color="#0891b2"/>
    <stop offset="100%" stop-color="#22d3ee"/>
  </linearGradient>
</defs>
<rect width="100%" height="100%" fill="#f7fafc"/>
<text x="{left}" y="42" font-family="Segoe UI, Microsoft YaHei, Arial" font-size="24" font-weight="700" fill="#0f172a">SoundGuard · 24 小时环境声学报告</text>
<text x="{left}" y="68" font-family="Segoe UI, Microsoft YaHei, Arial" font-size="13" fill="#64748b">{device} · 阈值 {threshold:.1} dB · {from} — {to}</text>
"##,
        device = xml_escape(device_name),
        from = local_label(from),
        to = local_label(to),
        threshold = threshold_db,
    );

    for step in 0..=5 {
        let value = min_y + (max_y - min_y) * (step as f64 / 5.0);
        let y = y_for(value);
        let _ = write!(
            svg,
            r##"<line x1="{left}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" stroke="#e2e8f0" stroke-width="1"/>
<text x="{label_x:.1}" y="{ty:.1}" text-anchor="end" font-family="Segoe UI, Arial" font-size="12" fill="#94a3b8">{value:.0}</text>
"##,
            left = left,
            y = y,
            x2 = left + plot_w,
            label_x = left - 12.0,
            ty = y + 4.0,
        );
    }

    let threshold_y = y_for(threshold_db);
    let _ = write!(
        svg,
        r##"<line x1="{left}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" stroke="#f97316" stroke-width="1.5" stroke-dasharray="7 6"/>
<rect x="{rx:.1}" y="{ry:.1}" width="104" height="22" rx="6" fill="#fff7ed" stroke="#fed7aa"/>
<text x="{tx:.1}" y="{ty:.1}" font-family="Segoe UI, Microsoft YaHei, Arial" font-size="11" fill="#c2410c">阈值 {threshold:.0} dB</text>
"##,
        left = left,
        y = threshold_y,
        x2 = left + plot_w,
        rx = left + plot_w - 112.0,
        ry = threshold_y - 28.0,
        tx = left + plot_w - 104.0,
        ty = threshold_y - 13.0,
        threshold = threshold_db,
    );

    if points.len() >= 2 {
        let coords: Vec<String> = points
            .iter()
            .map(|reading| {
                format!(
                    "{:.1},{:.1}",
                    x_for(reading.recorded_at),
                    y_for(reading.avg_db)
                )
            })
            .collect();
        let area = format!(
            "M {:.1},{:.1} L {} L {:.1},{:.1} Z",
            x_for(points[0].recorded_at),
            top + plot_h,
            coords.join(" L "),
            x_for(points[points.len() - 1].recorded_at),
            top + plot_h
        );
        let _ = write!(
            svg,
            r##"<path d="{area}" fill="url(#area)"/>
<polyline points="{}" fill="none" stroke="url(#line)" stroke-width="2.4" stroke-linejoin="round" stroke-linecap="round"/>
"##,
            coords.join(" ")
        );
    } else {
        let _ = write!(
            svg,
            r##"<text x="{cx:.1}" y="{cy:.1}" text-anchor="middle" font-family="Segoe UI, Microsoft YaHei, Arial" font-size="15" fill="#94a3b8">所选时间段暂无采集数据</text>
"##,
            cx = left + plot_w / 2.0,
            cy = top + plot_h / 2.0,
        );
    }

    let axis_y = top + plot_h + 24.0;
    let _ = write!(
        svg,
        r##"<line x1="{left}" y1="{by:.1}" x2="{bx:.1}" y2="{by:.1}" stroke="#cbd5e1"/>
<text x="{lx:.1}" y="{ay:.1}" font-family="Segoe UI, Arial" font-size="12" fill="#64748b">{from_label}</text>
<text x="{rx:.1}" y="{ay:.1}" text-anchor="end" font-family="Segoe UI, Arial" font-size="12" fill="#64748b">{to_label}</text>
<text x="{sx:.1}" y="{sy:.1}" font-family="Segoe UI, Microsoft YaHei, Arial" font-size="13" fill="#334155">平均 {avg} dB　·　峰值 {peak} dB　·　超限 {over} 次　·　样本 {samples}</text>
<rect x="{left}" y="{ty:.1}" width="9" height="9" rx="2" fill="#0891b2"/>
<text x="{tx:.1}" y="{ty2:.1}" font-family="Segoe UI, Microsoft YaHei, Arial" font-size="11" fill="#64748b">30 秒平均分贝</text>
"##,
        left = left,
        by = top + plot_h,
        bx = left + plot_w,
        lx = left,
        rx = left + plot_w,
        ay = axis_y,
        sy = height - 22.0,
        sx = left,
        from_label = local_label(from),
        to_label = local_label(to),
        avg = fmt_opt(stats.avg_db),
        peak = fmt_opt(stats.peak_db),
        over = stats.over_count,
        samples = stats.sample_count,
        ty = height - 22.0 - 13.0,
        tx = left + 16.0,
        ty2 = height - 20.0,
    );
    svg.push_str("</svg>");
    svg
}

fn fmt_opt(value: Option<f64>) -> String {
    value.map(|v| format!("{v:.1}")).unwrap_or_else(|| "--".to_string())
}

fn local_label(time: DateTime<Utc>) -> String {
    time.with_timezone(&Local).format("%m-%d %H:%M").to_string()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn html_escape(value: &str) -> String {
    xml_escape(value).replace('\'', "&#39;")
}

pub async fn send_daily_report(
    state: &AppState,
    device_id: &str,
) -> anyhow::Result<ReportResult> {
    let snapshot = state.store.snapshot().await;
    let device = snapshot
        .devices
        .iter()
        .find(|device| device.id == device_id)
        .ok_or_else(|| anyhow::anyhow!("设备不存在"))?
        .clone();
    let settings = crate::models::effective_settings(&snapshot.settings, &device);
    let to = Utc::now();
    let from = to - Duration::hours(24);
    let readings = state
        .store
        .read_readings_for(Some(device_id), from, to)
        .await?;
    if readings.is_empty() {
        return Err(anyhow::anyhow!("最近 24 小时没有采样数据，未生成日报"));
    }
    let stats = Stats::from_readings(&readings);
    let svg = render_svg(
        &device.name,
        settings.threshold_db,
        from,
        to,
        &readings,
        &stats,
    );
    let csv = readings_csv(&readings)?;
    let local_day = to.with_timezone(&Local).format("%Y-%m-%d").to_string();
    let recipients = crate::models::notification_recipients(
        &snapshot,
        &device.id,
        &settings.report_recipients,
    );
    if recipients.is_empty() {
        return Err(anyhow::anyhow!("没有可用的日报收件人"));
    }

    let mut alert = Alert::new(
        device.id.clone(),
        AlertKind::Report,
        "info",
        "24 小时环境声学日报",
        format!(
            "统计区间 {} — {}，平均 {} dB，峰值 {} dB，超限 {} 次。",
            local_label(from),
            local_label(to),
            fmt_opt(stats.avg_db),
            fmt_opt(stats.peak_db),
            stats.over_count
        ),
    );
    alert.email_status = if recipients.is_empty() {
        "disabled".to_string()
    } else {
        "pending".to_string()
    };
    let alert_id = alert.id.clone();
    state.store.push_alert(alert.clone()).await?;
    state.hub.broadcast_alert(&alert);

    let subject = format!(
        "【声垣日报】{local_day} · {} 环境声学报告",
        device.name
    );
    let text = format!(
        "声垣 SoundGuard · 24 小时环境声学报告\n\n设备：{}\n统计区间：{} — {}\n平均分贝：{} dB\n峰值：{} dB\n最低：{} dB\n超限次数：{} 次\n样本数：{}\n阈值：{:.1} dB\n\n附件包含原始数据 CSV 与统计图 SVG。\n",
        device.name,
        local_label(from),
        local_label(to),
        fmt_opt(stats.avg_db),
        fmt_opt(stats.peak_db),
        fmt_opt(stats.min_db),
        stats.over_count,
        stats.sample_count,
        settings.threshold_db,
    );
    let html = format!(
        r##"<div style="font-family:-apple-system,Segoe UI,Microsoft YaHei,Arial,sans-serif;background:#0b1220;padding:28px;color:#e2e8f0">
<div style="max-width:640px;margin:0 auto;background:#111a2e;border:1px solid #22304a;border-radius:18px;overflow:hidden">
  <div style="padding:26px 28px;background:linear-gradient(135deg,#0e7490,#155e75)">
    <div style="font-size:12px;letter-spacing:2px;color:#a5f3fc">SOUNDGUARD DAILY</div>
    <div style="font-size:24px;font-weight:700;margin-top:6px">环境声学日报</div>
    <div style="font-size:13px;color:#cffafe;margin-top:8px">{device}</div>
  </div>
  <div style="padding:24px 28px">
    <p style="color:#94a3b8;font-size:13px;margin:0 0 20px">统计区间：{from} — {to}</p>
    <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
      <tr>
        <td style="background:#0b1628;border:1px solid #22304a;border-radius:12px;padding:16px;text-align:center">
          <div style="font-size:12px;color:#64748b">平均分贝</div>
          <div style="font-size:28px;font-weight:700;color:#22d3ee;margin-top:6px">{avg}<span style="font-size:13px"> dB</span></div>
        </td>
        <td width="12"></td>
        <td style="background:#0b1628;border:1px solid #22304a;border-radius:12px;padding:16px;text-align:center">
          <div style="font-size:12px;color:#64748b">峰值</div>
          <div style="font-size:28px;font-weight:700;color:#fbbf24;margin-top:6px">{peak}<span style="font-size:13px"> dB</span></div>
        </td>
      </tr>
    </table>
    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-top:18px;font-size:13px;color:#cbd5e1">
      <tr><td style="padding:7px 0;color:#64748b">超限次数</td><td align="right">{over} 次</td></tr>
      <tr><td style="padding:7px 0;color:#64748b">最低分贝</td><td align="right">{min} dB</td></tr>
      <tr><td style="padding:7px 0;color:#64748b">采样点</td><td align="right">{samples}</td></tr>
      <tr><td style="padding:7px 0;color:#64748b">报警阈值</td><td align="right">{threshold:.1} dB</td></tr>
    </table>
    <p style="margin-top:22px;font-size:12px;line-height:1.7;color:#64748b">本邮件由声垣智能分贝监测系统自动生成。附件包含 24 小时原始数据表与统计图。</p>
  </div>
</div>
</div>"##,
        device = html_escape(&device.name),
        from = local_label(from),
        to = local_label(to),
        avg = fmt_opt(stats.avg_db),
        peak = fmt_opt(stats.peak_db),
        min = fmt_opt(stats.min_db),
        over = stats.over_count,
        samples = stats.sample_count,
        threshold = settings.threshold_db,
    );

    let safe_id: String = device
        .id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    let attachments = vec![
        MailAttachment {
            filename: format!("soundguard-{safe_id}-{local_day}.csv"),
            content_type: "text/csv".to_string(),
            bytes: csv,
        },
        MailAttachment {
            filename: format!("soundguard-{safe_id}-{local_day}.svg"),
            content_type: "image/svg+xml".to_string(),
            bytes: svg.into_bytes(),
        },
    ];

    let status = if recipients.is_empty() {
        "disabled".to_string()
    } else {
        let result = state
            .mailer
            .send(&recipients, &subject, &text, &html, attachments)
            .await;
        if let Some(error) = &result.error {
            error!("Daily report email status {}: {}", result.status, error);
        }
        result.status
    };
    if let Some(updated) = state
        .store
        .update_alert_email(&alert_id, &status, None)
        .await?
    {
        state.hub.broadcast_alert(&updated);
    }
    info!("Daily report generated for {local_day}; email status: {status}");
    Ok(ReportResult {
        device_id: device.id,
        device_name: device.name,
        recipients,
        email_status: status,
        alert_id,
        sample_count: readings.len(),
        period_from: from,
        period_to: to,
    })
}

pub fn spawn_daily_report_scheduler(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            ticker.tick().await;
            let snapshot = state.store.snapshot().await;
            let settings = &snapshot.settings;
            let has_user_email = snapshot
                .users
                .iter()
                .any(|user| !user.disabled && !user.email.trim().is_empty());
            if !settings.daily_report_enabled
                || (!has_user_email && settings.report_recipients.is_empty())
            {
                continue;
            }
            let (target_hour, target_minute) = settings
                .daily_report_time
                .split_once(':')
                .and_then(|(hour, minute)| {
                    Some((hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?))
                })
                .unwrap_or((8, 0));
            let now = Local::now();
            let now_minutes = now.hour() * 60 + now.minute();
            if now_minutes < target_hour * 60 + target_minute {
                continue;
            }
            let today = now.format("%Y-%m-%d").to_string();
            if snapshot.last_daily_report_date.as_deref() == Some(today.as_str()) {
                continue;
            }
            if let Err(error) = state
                .store
                .mutate(|state| state.last_daily_report_date = Some(today.clone()))
                .await
            {
                error!("Could not mark daily report date: {error}");
                continue;
            }
            for device in snapshot.devices.clone() {
                let device_id = device.id.clone();
                let from = Utc::now() - Duration::hours(24);
                let readings = state
                    .store
                    .read_readings_for(Some(&device_id), from, Utc::now())
                    .await
                    .unwrap_or_default();
                if readings.is_empty() {
                    continue;
                }
                match send_daily_report(&state, &device_id).await {
                    Ok(result) => info!(
                        "Scheduled report sent for {}: {} samples, email {}",
                        result.device_name, result.sample_count, result.email_status
                    ),
                    Err(error) => {
                        error!("Scheduled report failed for {device_id}: {error:#}")
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(offset_seconds: i64, db: f64) -> Reading {
        Reading {
            device_id: "demo".to_string(),
            recorded_at: Utc::now() + Duration::seconds(offset_seconds),
            avg_db: db,
            peak_db: db + 4.0,
            min_db: db - 4.0,
            over_threshold: db >= 65.0,
            duration_secs: 30,
            sample_count: 300,
        }
    }

    #[test]
    fn csv_contains_header_and_rows() {
        let csv = readings_csv(&[reading(0, 50.0)]).unwrap();
        let text = String::from_utf8(csv).unwrap();
        assert!(text.contains("avg_db"));
        assert!(text.contains("50.0"));
    }

    #[test]
    fn svg_is_well_formed_enough() {
        let readings = vec![reading(0, 42.0), reading(30, 66.0), reading(60, 58.0)];
        let svg = render_svg(
            "测试设备",
            65.0,
            Utc::now() - Duration::hours(1),
            Utc::now(),
            &readings,
            &Stats::from_readings(&readings),
        );
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("polyline"));
    }
}
