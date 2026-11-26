//! The desktop registers its existing catalogs with the shared translation API
//! (`argus_view_logic::catalog`, which embeds them).
pub use argus_view_logic::catalog::register;

/// Sets the language of all text. The component library keeps a catalog of
/// its own for what it draws itself (accessibility labels, the touch edit
/// menu): it follows the same tag, and falls back to English for the
/// languages it lacks.
pub fn set_locale(locale: argus_core::i18n::Locale) {
    argus_core::i18n::set_locale(locale);
    gpui_kit::component::set_locale(locale.tag());
}
