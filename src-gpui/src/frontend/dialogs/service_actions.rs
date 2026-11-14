//! Service actions that need a dialog: writing the start type of a packaged
//! service to the registry once the Service Control Manager refused it.
use super::*;

impl Desktop {
    /// Asks before writing the start type in `args` (the refused
    /// `set_service_start_type` request) to the service's registry key.
    pub(super) fn offer_registry_start_type(
        &mut self,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = text(args, "name").to_owned();
        let start_type = text(args, "startType").to_owned();
        let display = match text(args, "display") {
            "" => name.clone(),
            display => display.to_owned(),
        };
        let display_arg = display.clone();
        let chosen = label(&format!("services.startType.{start_type}"));
        self.ask(
            label("services.packaged.title"),
            argus_core::i18n::format(
                "services.packaged.body",
                &[
                    ("name", &display as &dyn std::fmt::Display),
                    ("type", &chosen as &dyn std::fmt::Display),
                ],
            ),
            label("services.packaged.confirm"),
            false,
            move |this, window, cx| {
                this.request(
                    "set_service_start_type_registry",
                    json!({"name":name,"startType":start_type,"display":display_arg}),
                    None,
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }
}
