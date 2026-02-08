use i18n_embed::{
    fluent::{fluent_language_loader, FluentLanguageLoader},
    DefaultLocalizer, LanguageLoader, Localizer,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

pub static LANGUAGE_LOADER: once_cell::sync::Lazy<FluentLanguageLoader> =
    once_cell::sync::Lazy::new(|| {
        let loader: FluentLanguageLoader = fluent_language_loader!();
        loader
            .load_fallback_language(&Localizations)
            .expect("Error while loading fallback language");
        let localizer = DefaultLocalizer::new(&loader, &Localizations);
        let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
        let _ = localizer.select(&requested_languages);
        loader
    });

#[macro_export]
macro_rules! fl {
    ($message_id:literal) => {{
        i18n_embed_fl::fl!($crate::localize::LANGUAGE_LOADER, $message_id)
    }};
    ($message_id:literal, $($args:expr),*) => {{
        i18n_embed_fl::fl!($crate::localize::LANGUAGE_LOADER, $message_id, $($args), *)
    }};
}
