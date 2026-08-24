use std::{collections::HashMap, time::Duration};

use common::{
    AllTimeData, DatabaseEntry, ProcessData, TotalData, UsageSummary, usage::cached_usage_summary,
};
use iced::{
    Alignment, Element, Length, Padding,
    alignment::{Horizontal, Vertical},
    widget::{Column, Container, Row, Scrollable, Text, button, rule},
};

use crate::{
    components::{helpers::no_data_placeholder, sensor_state::SensorState},
    message::Message,
    styles::{
        button::ButtonStyle,
        container::ContainerStyle,
        scrollable::ScrollableStyle,
        style_constants::{
            FONT_BOLD, FONT_SIZE_BODY, FONT_SIZE_LARGE, FONT_SIZE_SMALL, FONT_SIZE_SUBTITLE, FONT_SIZE_TITLE,
            PADDING_LARGE, PADDING_MEDIUM, SPACING_LARGE, SPACING_SMALL, SPACING_XLARGE,
        },
        text::TextStyle,
    },
    themes::AppTheme,
    translations::{
        all_time, current_power_consumption, electricity_bill, emissions, format_emissions, format_energy,
        format_number, zero_carbon_intensity_warning,
    },
    types::{AppLanguage, CarbonIntensity, ElectricityCost},
};

const SUMMARY_CACHE_AGE: Duration = Duration::from_secs(5);

/// Dashboard page showing current power, calendar energy summaries, charts, and process details.
pub struct DashboardPage;

impl DashboardPage {
    pub fn view<'a>(
        &'a self,
        sensors: &'a HashMap<String, SensorState>,
        all_time_data: &'a AllTimeData,
        language: AppLanguage,
        carbon_intensity: CarbonIntensity,
        electricity_cost: ElectricityCost,
    ) -> Element<'a, Message, AppTheme> {
        let usage = cached_usage_summary(SUMMARY_CACHE_AGE);

        let content = Column::new()
            .spacing(SPACING_LARGE)
            .padding(Padding::from(PADDING_LARGE))
            .width(Length::Fill)
            .height(Length::Fill)
            .push(self.view_power_summary(sensors, all_time_data, language, carbon_intensity, electricity_cost))
            .push(self.view_calendar_summary(&usage, language, carbon_intensity, electricity_cost));

        let additional_content = Column::new()
            .spacing(SPACING_XLARGE)
            .padding(Padding::from(PADDING_LARGE))
            .width(Length::Fill)
            .height(Length::Fill)
            .push(self.chart_or_placeholder(sensors, None, TotalData::table_name_static(), 300.0, false, language))
            .push(self.view_process_summary(sensors, language))
            .push(self.view_component_cards(sensors));

        content
            .push(
                Scrollable::new(additional_content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .class(ScrollableStyle::Standard),
            )
            .into()
    }

    fn view_power_summary<'a>(
        &'a self,
        sensors: &'a HashMap<String, SensorState>,
        all_time_data: &'a AllTimeData,
        language: AppLanguage,
        carbon_intensity: CarbonIntensity,
        electricity_cost: ElectricityCost,
    ) -> Element<'a, Message, AppTheme> {
        let raw_power = sensors
            .get(TotalData::table_name_static())
            .and_then(|c| c.get_latest_reading())
            .and_then(|data| data.total_energy())
            .map(|energy| energy.as_watts_for_seconds(1.0))
            .unwrap_or(0.0);
        let power_value = format_number(raw_power, 1, language);
        let current_cost_per_hour = raw_power / 1000.0 * electricity_cost.price_per_kwh;

        let main = Column::new()
            .width(Length::FillPortion(1))
            .spacing(SPACING_SMALL)
            .align_x(Alignment::Center)
            .push(
                Text::new(current_power_consumption(language))
                    .size(FONT_SIZE_SUBTITLE)
                    .font(FONT_BOLD)
                    .class(TextStyle::Subtitle),
            )
            .push(
                Row::new()
                    .align_y(Alignment::End)
                    .spacing(4)
                    .push(
                        Text::new(power_value)
                            .size(FONT_SIZE_LARGE)
                            .font(FONT_BOLD)
                            .class(TextStyle::Primary),
                    )
                    .push(Text::new("W").size(FONT_SIZE_TITLE).class(TextStyle::Muted)),
            )
            .push(
                Text::new(format!(
                    "≈ {} {} / {}",
                    format_number(current_cost_per_hour.max(0.0), 2, language),
                    electricity_cost.currency_symbol,
                    hour_label(language)
                ))
                .size(FONT_SIZE_BODY)
                .class(TextStyle::Muted),
            );

        let total_energy_wh = all_time_data
            .components
            .get(TotalData::table_name_static())
            .copied()
            .map(|energy| energy.as_watt_hours())
            .unwrap_or(0.0);

        let carbon_grams = wh_to_co2_grams(total_energy_wh, carbon_intensity.g_per_kwh);
        let bill = (total_energy_wh / 1000.0) * electricity_cost.price_per_kwh;

        let (energy_val, energy_unit) = format_energy(total_energy_wh, language);
        let (emissions_val, emissions_unit) = format_emissions(carbon_grams, language);
        let bill_val = format_number(bill.max(0.0), 2, language);

        let help_button = button(Text::new("?").size(FONT_SIZE_BODY).font(FONT_BOLD))
            .class(ButtonStyle::InfoHelp)
            .on_press(Message::OpenInfoModal("carbon_emissions".to_string()))
            .padding(Padding::from([2, 8]));

        let mut metrics_left = Column::new()
            .spacing(SPACING_SMALL)
            .align_x(Alignment::Center)
            .push(
                Row::new()
                    .push(metric_tile(
                        all_time(language),
                        energy_val,
                        energy_unit,
                        TextStyle::Secondary,
                    ))
                    .push(Text::new(" ").size(FONT_SIZE_BODY).width(Length::Fixed(24.0))),
            )
            .push(
                Row::new()
                    .push(metric_tile(
                        emissions(language),
                        emissions_val,
                        emissions_unit,
                        TextStyle::Tertiary,
                    ))
                    .align_y(Alignment::Center)
                    .push(help_button),
            );

        if carbon_intensity.g_per_kwh == 0.0 {
            metrics_left = metrics_left.push(
                Text::new(zero_carbon_intensity_warning(language))
                    .size(FONT_SIZE_BODY)
                    .align_x(Alignment::Center)
                    .class(TextStyle::Tertiary),
            );
        }

        let bill_col = Column::new()
            .spacing(SPACING_SMALL)
            .align_x(Alignment::Center)
            .push(metric_tile(
                electricity_bill(language),
                bill_val,
                electricity_cost.currency_symbol,
                TextStyle::Primary,
            ));

        let side = Row::new()
            .width(Length::FillPortion(1))
            .align_y(Alignment::Center)
            .spacing(SPACING_SMALL)
            .push(metrics_left.width(Length::FillPortion(1)))
            .push(bill_col.width(Length::FillPortion(1)));

        let content = Row::new()
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .push(main)
            .push(rule::vertical(1))
            .push(side);

        Container::new(content)
            .width(Length::Fill)
            .height(Length::Shrink)
            .padding(Padding::from(PADDING_MEDIUM))
            .class(ContainerStyle::PowerCard)
            .into()
    }

    fn view_calendar_summary<'a>(
        &'a self,
        usage: &UsageSummary,
        language: AppLanguage,
        carbon_intensity: CarbonIntensity,
        electricity_cost: ElectricityCost,
    ) -> Element<'a, Message, AppTheme> {
        let today_cost = cost_for_energy(usage.today_energy_wh, electricity_cost);
        let month_cost = cost_for_energy(usage.month_energy_wh, electricity_cost);
        let projected_cost = cost_for_energy(usage.projected_month_energy_wh, electricity_cost);
        let daily_cost = cost_for_energy(usage.average_daily_energy_wh, electricity_cost);
        let active_hour_cost = cost_for_energy(usage.average_active_hour_energy_wh, electricity_cost);

        let top_row = Row::new()
            .spacing(SPACING_LARGE)
            .width(Length::Fill)
            .push(summary_tile(
                today_label(language),
                usage.today_energy_wh,
                today_cost,
                electricity_cost,
                language,
                TextStyle::Secondary,
            ))
            .push(summary_tile(
                this_month_label(language),
                usage.month_energy_wh,
                month_cost,
                electricity_cost,
                language,
                TextStyle::Secondary,
            ))
            .push(summary_tile(
                projected_month_label(language),
                usage.projected_month_energy_wh,
                projected_cost,
                electricity_cost,
                language,
                TextStyle::Primary,
            ));

        let today_co2 = wh_to_co2_grams(usage.today_energy_wh, carbon_intensity.g_per_kwh);
        let (co2_value, co2_unit) = format_emissions(today_co2, language);
        let basis_days = format_number(usage.projection_basis_days, 1, language);

        let bottom_row = Row::new()
            .spacing(SPACING_LARGE)
            .width(Length::Fill)
            .push(summary_tile(
                recent_daily_average_label(language),
                usage.average_daily_energy_wh,
                daily_cost,
                electricity_cost,
                language,
                TextStyle::Secondary,
            ))
            .push(summary_tile(
                average_active_hour_label(language),
                usage.average_active_hour_energy_wh,
                active_hour_cost,
                electricity_cost,
                language,
                TextStyle::Secondary,
            ))
            .push(
                Container::new(
                    Column::new()
                        .spacing(SPACING_SMALL)
                        .align_x(Alignment::Center)
                        .push(
                            Text::new(monitored_today_label(language))
                                .size(FONT_SIZE_BODY)
                                .font(FONT_BOLD)
                                .class(TextStyle::Subtitle),
                        )
                        .push(
                            Text::new(format_duration(usage.today_monitored_seconds))
                                .size(FONT_SIZE_SUBTITLE)
                                .font(FONT_BOLD)
                                .class(TextStyle::Primary),
                        )
                        .push(
                            Text::new(format!("{} {} {}", co2_value, co2_unit, emissions_today_suffix(language)))
                                .size(FONT_SIZE_SMALL)
                                .class(TextStyle::Muted),
                        )
                        .push(
                            Text::new(format!("{} {}", basis_days, projection_basis_suffix(language)))
                                .size(FONT_SIZE_SMALL)
                                .class(TextStyle::Muted),
                        ),
                )
                .width(Length::FillPortion(1))
                .padding(Padding::from(PADDING_MEDIUM))
                .class(ContainerStyle::Card),
            );

        Column::new().spacing(SPACING_LARGE).push(top_row).push(bottom_row).into()
    }

    fn view_process_summary<'a>(
        &'a self,
        sensors: &'a HashMap<String, SensorState>,
        language: AppLanguage,
    ) -> Element<'a, Message, AppTheme> {
        let process_data = sensors.get(ProcessData::table_name_static());

        if let Some(process_card) = process_data.and_then(|p| Some(p.sensor_visual_card(None, 300.0, false))) {
            process_card
        } else {
            no_data_placeholder(language)
        }
    }

    fn view_component_cards<'a>(&'a self, sensors: &'a HashMap<String, SensorState>) -> Element<'a, Message, AppTheme> {
        let mut column = Column::new().spacing(SPACING_LARGE).width(Length::Fill);

        let mut sensors: Vec<(&String, &SensorState)> = sensors
            .iter()
            .filter(|(table_name, _)| {
                *table_name != TotalData::table_name_static() && *table_name != ProcessData::table_name_static()
            })
            .collect();

        fn priority(name: &str) -> usize {
            let lower = name.to_lowercase();
            if lower.contains("cpu") {
                0
            } else if lower.contains("gpu") {
                1
            } else if lower.contains("ram") {
                2
            } else if lower.contains("disk") {
                3
            } else if lower.contains("network") {
                4
            } else {
                5
            }
        }

        sensors.sort_by_key(|(name, _)| (priority(name.as_str()), *name));

        let mut row = Row::new().spacing(SPACING_LARGE).width(Length::Fill);
        let mut items_in_row = 0usize;

        for (i, (_, sensor)) in sensors.into_iter().enumerate() {
            let card = sensor.sensor_visual_card(None, 200.0, true);

            row = row.push(card);
            items_in_row += 1;

            if i % 2 == 1 {
                column = column.push(row);
                row = Row::new().spacing(SPACING_LARGE).width(Length::Fill);
                items_in_row = 0;
            }
        }

        if items_in_row % 2 == 1 {
            row = row.push(Row::new().spacing(SPACING_LARGE).width(Length::Fill));
        }

        if items_in_row > 0 {
            column = column.push(row);
        }

        Container::new(column)
            .width(Length::Fill)
            .padding(Padding::from(PADDING_LARGE))
            .class(ContainerStyle::Card)
            .into()
    }

    fn chart_or_placeholder<'a>(
        &'a self,
        sensors: &'a HashMap<String, SensorState>,
        title: Option<&'static str>,
        table_name: &str,
        height: f32,
        show_usage: bool,
        language: AppLanguage,
    ) -> Element<'a, Message, AppTheme> {
        sensors
            .get(table_name)
            .map(|c| c.sensor_visual_card(title, height, show_usage))
            .unwrap_or_else(|| no_data_placeholder(language))
    }
}

fn metric_tile<'a>(
    label: &'a str,
    value: String,
    unit: &'a str,
    value_style: TextStyle,
) -> Element<'a, Message, AppTheme> {
    let value_row = Row::new()
        .spacing(4)
        .align_y(Alignment::End)
        .push(
            Text::new(value)
                .size(FONT_SIZE_SUBTITLE)
                .font(FONT_BOLD)
                .class(value_style),
        )
        .push(Text::new(unit).size(FONT_SIZE_BODY).class(TextStyle::Muted));

    Container::new(
        Column::new()
            .padding(Padding::from(PADDING_MEDIUM))
            .spacing(2)
            .align_x(Alignment::Center)
            .push(
                Text::new(label)
                    .size(FONT_SIZE_BODY)
                    .font(FONT_BOLD)
                    .class(TextStyle::Subtitle),
            )
            .push(value_row),
    )
    .width(Length::Fill)
    .align_x(Horizontal::Center)
    .align_y(Vertical::Center)
    .into()
}

fn summary_tile<'a>(
    label: &'a str,
    energy_wh: f64,
    cost: f64,
    electricity_cost: ElectricityCost,
    language: AppLanguage,
    value_style: TextStyle,
) -> Element<'a, Message, AppTheme> {
    let (energy_value, energy_unit) = format_energy(energy_wh.max(0.0), language);
    let cost_value = format_number(cost.max(0.0), 2, language);

    Container::new(
        Column::new()
            .spacing(SPACING_SMALL)
            .align_x(Alignment::Center)
            .push(
                Text::new(label)
                    .size(FONT_SIZE_BODY)
                    .font(FONT_BOLD)
                    .class(TextStyle::Subtitle),
            )
            .push(
                Text::new(format!("{} {}", energy_value, energy_unit))
                    .size(FONT_SIZE_SUBTITLE)
                    .font(FONT_BOLD)
                    .class(value_style),
            )
            .push(
                Text::new(format!("{} {}", cost_value, electricity_cost.currency_symbol))
                    .size(FONT_SIZE_BODY)
                    .class(TextStyle::Muted),
            ),
    )
    .width(Length::FillPortion(1))
    .padding(Padding::from(PADDING_MEDIUM))
    .class(ContainerStyle::Card)
    .into()
}

fn cost_for_energy(energy_wh: f64, electricity_cost: ElectricityCost) -> f64 {
    energy_wh.max(0.0) / 1000.0 * electricity_cost.price_per_kwh.max(0.0)
}

fn wh_to_co2_grams(energy_wh: f64, intensity_g_per_kwh: f64) -> f64 {
    (energy_wh / 1000.0) * intensity_g_per_kwh
}

fn format_duration(seconds: f64) -> String {
    let total_minutes = (seconds.max(0.0) / 60.0).floor() as u64;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    format!("{}h {:02}m", hours, minutes)
}

fn hour_label(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "hour",
        AppLanguage::French => "heure",
        AppLanguage::Chinese => "小时",
        AppLanguage::Romanian => "oră",
    }
}

fn today_label(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "Today",
        AppLanguage::French => "Aujourd’hui",
        AppLanguage::Chinese => "今天",
        AppLanguage::Romanian => "Astăzi",
    }
}

fn this_month_label(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "This month",
        AppLanguage::French => "Ce mois-ci",
        AppLanguage::Chinese => "本月",
        AppLanguage::Romanian => "Luna aceasta",
    }
}

fn projected_month_label(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "Projected month",
        AppLanguage::French => "Projection mensuelle",
        AppLanguage::Chinese => "月度预测",
        AppLanguage::Romanian => "Proiecție lunară",
    }
}

fn recent_daily_average_label(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "Recent daily average",
        AppLanguage::French => "Moyenne quotidienne récente",
        AppLanguage::Chinese => "近期日均",
        AppLanguage::Romanian => "Media zilnică recentă",
    }
}

fn average_active_hour_label(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "Average active hour",
        AppLanguage::French => "Heure active moyenne",
        AppLanguage::Chinese => "平均运行小时",
        AppLanguage::Romanian => "Oră activă medie",
    }
}

fn monitored_today_label(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "Monitored today",
        AppLanguage::French => "Suivi aujourd’hui",
        AppLanguage::Chinese => "今日监测时长",
        AppLanguage::Romanian => "Monitorizat astăzi",
    }
}

fn emissions_today_suffix(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "CO₂ today",
        AppLanguage::French => "CO₂ aujourd’hui",
        AppLanguage::Chinese => "今日 CO₂",
        AppLanguage::Romanian => "CO₂ astăzi",
    }
}

fn projection_basis_suffix(language: AppLanguage) -> &'static str {
    match language {
        AppLanguage::English => "days of history",
        AppLanguage::French => "jours d’historique",
        AppLanguage::Chinese => "天历史数据",
        AppLanguage::Romanian => "zile de istoric",
    }
}
