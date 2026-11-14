// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use acl_core::task::StageSnapshot;
use acl_core::task::StageStatus;
use libadwaita::prelude::*;
use relm4::SimpleComponent;
use relm4::prelude::*;

#[derive(Debug, Clone)]
pub struct StageRow {
    stage: StageSnapshot,
}

impl StageRow {
    fn status_icon(&self) -> &'static str {
        match self.stage.status {
            StageStatus::Pending | StageStatus::Running => "content-loading-symbolic",
            StageStatus::Done => "object-select-symbolic",
            StageStatus::Cancelled => "process-stop-symbolic",
            StageStatus::Failed => "dialog-error-symbolic",
        }
    }

    fn status_css(&self) -> &'static [&'static str] {
        match self.stage.status {
            StageStatus::Pending | StageStatus::Running | StageStatus::Cancelled => &["dim-label"],
            StageStatus::Done => &["success"],
            StageStatus::Failed => &["error"],
        }
    }

    fn fraction(&self) -> f64 {
        if self.stage.total == 0 {
            0.0
        } else {
            (self.stage.done as f64 / self.stage.total as f64).clamp(0.0, 1.0)
        }
    }
}

#[relm4::component(pub)]
impl SimpleComponent for StageRow {
    type Init = StageSnapshot;
    type Input = StageSnapshot;
    type Output = ();

    view! {
        #[root]
        adw::ActionRow {
            #[watch]
            set_title: &model.stage.label,

            add_prefix = &gtk::Box {
                set_valign: gtk::Align::Center,
                set_width_request: 16,

                adw::Spinner {
                    #[watch]
                    set_visible: model.stage.status == StageStatus::Running,
                },

                gtk::Image {
                    #[watch]
                    set_visible: model.stage.status != StageStatus::Running,
                    #[watch]
                    set_icon_name: Some(model.status_icon()),
                    #[watch]
                    set_css_classes: model.status_css(),
                }

            },

            add_suffix = &gtk::Label {
                set_valign: gtk::Align::Center,
                add_css_class: "numeric",
                add_css_class: "dim-label",
                #[watch]
                set_visible: model.stage.total > 1,
                #[watch]
                set_label: &format!("{} / {}", model.stage.done, model.stage.total),
            },


            add_suffix = &gtk::ProgressBar {
                set_valign: gtk::Align::Center,
                set_width_request: 120,
                #[watch]
                set_visible: model.stage.total > 1,
                #[watch]
                set_fraction: model.fraction(),
            }
        }
    }

    fn init(
        stage: Self::Init,
        root: Self::Root,
        _sender: relm4::prelude::ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self { stage };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        self.stage = msg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_with(status: StageStatus, done: u64, total: u64) -> StageRow {
        StageRow {
            stage: StageSnapshot {
                label: "Test stage".to_string(),
                status,
                done,
                total,
                bytes_per_sec: None,
            },
        }
    }

    #[test]
    fn test_stage_row_status_icon_and_css() {
        assert_eq!(
            row_with(StageStatus::Pending, 0, 0).status_icon(),
            "content-loading-symbolic"
        );
        assert_eq!(
            row_with(StageStatus::Running, 0, 0).status_icon(),
            "content-loading-symbolic"
        );
        assert_eq!(
            row_with(StageStatus::Done, 0, 0).status_icon(),
            "object-select-symbolic"
        );
        assert_eq!(
            row_with(StageStatus::Cancelled, 0, 0).status_icon(),
            "process-stop-symbolic"
        );
        assert_eq!(
            row_with(StageStatus::Failed, 0, 0).status_icon(),
            "dialog-error-symbolic"
        );

        assert_eq!(
            row_with(StageStatus::Pending, 0, 0).status_css(),
            &["dim-label"]
        );
        assert_eq!(
            row_with(StageStatus::Running, 0, 0).status_css(),
            &["dim-label"]
        );
        assert_eq!(
            row_with(StageStatus::Cancelled, 0, 0).status_css(),
            &["dim-label"]
        );
        assert_eq!(row_with(StageStatus::Done, 0, 0).status_css(), &["success"]);
        assert_eq!(row_with(StageStatus::Failed, 0, 0).status_css(), &["error"]);
    }

    #[test]
    fn test_stage_row_fraction() {
        assert_eq!(row_with(StageStatus::Pending, 0, 0).fraction(), 0.0);
        assert_eq!(row_with(StageStatus::Pending, 5, 0).fraction(), 0.0);
        assert!((row_with(StageStatus::Running, 3, 10).fraction() - 0.3).abs() < f64::EPSILON);
        assert_eq!(row_with(StageStatus::Running, 15, 10).fraction(), 1.0);
    }
}
