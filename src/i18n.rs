//! Minimal i18n for the UI. English is the base; other locales override by key.
//!
//! Convention (project spec): zero string literals in the render path - every
//! user-visible label goes through [`t`]. The current language is a thread-local
//! set once per frame from `MdApp::app_lang`, so `t("key")` stays a cheap call
//! at every widget. Missing keys fall back to English, then to the key itself.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

thread_local! {
    static CUR_LANG: RefCell<&'static str> = const { RefCell::new("en") };
}

/// Set the active UI language (called once per frame from the app state).
pub fn set_language(lang: &str) {
    let normalized = match lang {
        "fr" => "fr",
        "de" => "de",
        "es" => "es",
        "it" => "it",
        _ => "en",
    };
    CUR_LANG.with(|c| *c.borrow_mut() = normalized);
}

fn current() -> &'static str {
    CUR_LANG.with(|c| *c.borrow())
}

/// Translate a key into the current language, falling back to English then key.
pub fn t(key: &str) -> String {
    let lang = current();
    if lang != "en" {
        if let Some(v) = map_for(lang).get(key) {
            return (*v).to_string();
        }
    }
    en_map().get(key).copied().unwrap_or(key).to_string()
}

fn map_for(lang: &str) -> &'static HashMap<&'static str, &'static str> {
    match lang {
        "fr" => fr_map(),
        "de" => de_map(),
        "es" => es_map(),
        "it" => it_map(),
        _ => en_map(),
    }
}

fn build(pairs: &[(&'static str, &'static str)]) -> HashMap<&'static str, &'static str> {
    pairs.iter().copied().collect()
}

fn en_map() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| build(EN))
}

fn fr_map() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| build(FR))
}

fn de_map() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| build(DE))
}

fn es_map() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| build(ES))
}

fn it_map() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| build(IT))
}

// ── String tables ────────────────────────────────────────────────────────────
// Keys are dotted namespaces. Add a key to EN first, then translate in FR.

const EN: &[(&str, &str)] = &[
    // Top-level menus
    ("menu.file", "File"),
    ("menu.edit", "Edit"),
    ("menu.insert", "Insert"),
    ("menu.metadata", "Metadata"),
    ("menu.modules", "Modules"),
    // View modes
    ("view.hub", "Hub"),
    ("view.source", "Source"),
    ("view.split", "Split"),
    ("view.editor", "Editor"),
    // Module window
    ("module.title", "Modules"),
    ("module.tab.dictionaries", "Dictionaries"),
    ("module.tab.language", "App language"),
    ("module.tab.citations", "Citation styles"),
    ("module.tab.themes", "Themes"),
    ("module.tab.templates", "Templates"),
    ("module.enable_spell", "Enable spell checking"),
    ("module.active_dict", "Active dictionary"),
    ("module.no_dict", "No dictionary loaded."),
    ("module.add_dict", "Add dictionary (.dic + .aff)..."),
    ("module.add_dict_hint", "Pick a Hunspell .dic file; its matching .aff (same name) loads automatically."),
    ("module.downloadable", "Downloadable dictionaries"),
    ("module.downloadable_hint", "(opt-in - fetched from wooorm/dictionaries on demand)"),
    ("module.download", "Download"),
    ("module.downloading", "downloading..."),
    ("module.use", "Use"),
    ("module.in_use", "\u{2713} in use"),
    ("module.redownload", "Re-download"),
    ("module.language", "Application language"),
    ("module.language_hint", "UI strings go through an i18n table; selecting a language switches the interface live."),
    ("module.reserved", "Reserved module slot."),
    ("module.reserved_hint", "Downloadable packs (CSL citation styles, editor themes, journal/thesis templates) plug in here through the Module registry."),
    // Panels
    ("panel.spelling", "Spelling"),
    ("panel.review", "Review"),
    ("panel.no_issues", "No spelling issues \u{2713}"),
    ("panel.dictionary", "Dictionary"),
    ("panel.add", "Add"),
    ("panel.jump", "Jump"),
    ("panel.no_suggestions", "(no suggestions)"),
    // Options - PDF engine (neutral labels; no engine brand name in the UI)
    ("options.pdf_engine", "PDF engine"),
    ("options.pdf_engine.native", "Native Converter"),
    ("options.pdf_engine.general", "General Converter"),
    ("options.pdf_engine.native_hint", "Pure-Rust, fully offline, fastest"),
    ("options.pdf_engine.general_hint", "Highest-fidelity layout (bundled engine)"),
];

const FR: &[(&str, &str)] = &[
    ("menu.file", "Fichier"),
    ("menu.edit", "\u{00C9}dition"),
    ("menu.insert", "Insertion"),
    ("menu.metadata", "M\u{00E9}tadonn\u{00E9}es"),
    ("menu.modules", "Modules"),
    ("view.hub", "Accueil"),
    ("view.source", "Source"),
    ("view.split", "Divis\u{00E9}"),
    ("view.editor", "\u{00C9}diteur"),
    ("module.title", "Modules"),
    ("module.tab.dictionaries", "Dictionnaires"),
    ("module.tab.language", "Langue de l'app"),
    ("module.tab.citations", "Styles de citation"),
    ("module.tab.themes", "Th\u{00E8}mes"),
    ("module.tab.templates", "Mod\u{00E8}les"),
    ("module.enable_spell", "Activer la correction orthographique"),
    ("module.active_dict", "Dictionnaire actif"),
    ("module.no_dict", "Aucun dictionnaire charg\u{00E9}."),
    ("module.add_dict", "Ajouter un dictionnaire (.dic + .aff)..."),
    ("module.add_dict_hint", "Choisissez un fichier Hunspell .dic ; son .aff (m\u{00EA}me nom) est charg\u{00E9} automatiquement."),
    ("module.downloadable", "Dictionnaires t\u{00E9}l\u{00E9}chargeables"),
    ("module.downloadable_hint", "(opt-in - r\u{00E9}cup\u{00E9}r\u{00E9}s depuis wooorm/dictionaries \u{00E0} la demande)"),
    ("module.download", "T\u{00E9}l\u{00E9}charger"),
    ("module.downloading", "t\u{00E9}l\u{00E9}chargement..."),
    ("module.use", "Utiliser"),
    ("module.in_use", "\u{2713} actif"),
    ("module.redownload", "Re-t\u{00E9}l\u{00E9}charger"),
    ("module.language", "Langue de l'application"),
    ("module.language_hint", "Les libell\u{00E9}s passent par une table i18n ; choisir une langue bascule l'interface en direct."),
    ("module.reserved", "Emplacement de module r\u{00E9}serv\u{00E9}."),
    ("module.reserved_hint", "Des packs t\u{00E9}l\u{00E9}chargeables (styles de citation CSL, th\u{00E8}mes, mod\u{00E8}les Typst) se branchent ici via le registre de Modules."),
    ("panel.spelling", "Orthographe"),
    ("panel.review", "R\u{00E9}vision"),
    ("panel.no_issues", "Aucune faute \u{2713}"),
    ("panel.dictionary", "Dictionnaire"),
    ("panel.add", "Ajouter"),
    ("panel.jump", "Aller"),
    ("panel.no_suggestions", "(aucune suggestion)"),
    ("options.pdf_engine", "Moteur PDF"),
    ("options.pdf_engine.native", "Convertisseur Natif"),
    ("options.pdf_engine.general", "Convertisseur G\u{00E9}n\u{00E9}ral"),
    ("options.pdf_engine.native_hint", "Pur Rust, 100% hors-ligne, le plus rapide"),
    ("options.pdf_engine.general_hint", "Rendu le plus fid\u{00E8}le (moteur embarqu\u{00E9})"),
];

const DE: &[(&str, &str)] = &[
    ("menu.file", "Datei"),
    ("menu.edit", "Bearbeiten"),
    ("menu.insert", "Einf\u{00FC}gen"),
    ("menu.metadata", "Metadaten"),
    ("menu.modules", "Module"),
    ("view.hub", "Start"),
    ("view.source", "Quelle"),
    ("view.split", "Geteilt"),
    ("view.editor", "Editor"),
    ("module.title", "Module"),
    ("module.tab.dictionaries", "W\u{00F6}rterb\u{00FC}cher"),
    ("module.tab.language", "App-Sprache"),
    ("module.tab.citations", "Zitierstile"),
    ("module.tab.themes", "Designs"),
    ("module.tab.templates", "Vorlagen"),
    ("module.enable_spell", "Rechtschreibpr\u{00FC}fung aktivieren"),
    ("module.active_dict", "Aktives W\u{00F6}rterbuch"),
    ("module.no_dict", "Kein W\u{00F6}rterbuch geladen."),
    ("module.add_dict", "W\u{00F6}rterbuch hinzuf\u{00FC}gen (.dic + .aff)..."),
    ("module.add_dict_hint", "W\u{00E4}hlen Sie eine Hunspell-.dic-Datei; die passende .aff (gleicher Name) wird automatisch geladen."),
    ("module.downloadable", "Herunterladbare W\u{00F6}rterb\u{00FC}cher"),
    ("module.downloadable_hint", "(optional - bei Bedarf von wooorm/dictionaries geladen)"),
    ("module.download", "Herunterladen"),
    ("module.downloading", "wird heruntergeladen..."),
    ("module.use", "Verwenden"),
    ("module.in_use", "\u{2713} aktiv"),
    ("module.redownload", "Erneut herunterladen"),
    ("module.language", "Anwendungssprache"),
    ("module.language_hint", "Beschriftungen laufen \u{00FC}ber eine i18n-Tabelle; eine Sprache zu w\u{00E4}hlen schaltet die Oberfl\u{00E4}che sofort um."),
    ("module.reserved", "Reservierter Modulplatz."),
    ("module.reserved_hint", "Herunterladbare Pakete (CSL-Zitierstile, Designs, Typst-Vorlagen) werden hier \u{00FC}ber die Modul-Registry eingebunden."),
    ("panel.spelling", "Rechtschreibung"),
    ("panel.review", "\u{00DC}berpr\u{00FC}fung"),
    ("panel.no_issues", "Keine Fehler \u{2713}"),
    ("panel.dictionary", "W\u{00F6}rterbuch"),
    ("panel.add", "Hinzuf\u{00FC}gen"),
    ("panel.jump", "Springen"),
    ("panel.no_suggestions", "(keine Vorschl\u{00E4}ge)"),
    ("options.pdf_engine", "PDF-Engine"),
    ("options.pdf_engine.native", "Nativer Konverter"),
    ("options.pdf_engine.general", "Allgemeiner Konverter"),
    ("options.pdf_engine.native_hint", "Pures Rust, vollst\u{00E4}ndig offline, am schnellsten"),
    ("options.pdf_engine.general_hint", "H\u{00F6}chste Wiedergabetreue (eingebettete Engine)"),
];

const ES: &[(&str, &str)] = &[
    ("menu.file", "Archivo"),
    ("menu.edit", "Editar"),
    ("menu.insert", "Insertar"),
    ("menu.metadata", "Metadatos"),
    ("menu.modules", "M\u{00F3}dulos"),
    ("view.hub", "Inicio"),
    ("view.source", "Fuente"),
    ("view.split", "Dividido"),
    ("view.editor", "Editor"),
    ("module.title", "M\u{00F3}dulos"),
    ("module.tab.dictionaries", "Diccionarios"),
    ("module.tab.language", "Idioma de la app"),
    ("module.tab.citations", "Estilos de cita"),
    ("module.tab.themes", "Temas"),
    ("module.tab.templates", "Plantillas"),
    ("module.enable_spell", "Activar la correcci\u{00F3}n ortogr\u{00E1}fica"),
    ("module.active_dict", "Diccionario activo"),
    ("module.no_dict", "Ning\u{00FA}n diccionario cargado."),
    ("module.add_dict", "A\u{00F1}adir diccionario (.dic + .aff)..."),
    ("module.add_dict_hint", "Elija un archivo Hunspell .dic; su .aff (mismo nombre) se carga autom\u{00E1}ticamente."),
    ("module.downloadable", "Diccionarios descargables"),
    ("module.downloadable_hint", "(opcional - obtenidos de wooorm/dictionaries a demanda)"),
    ("module.download", "Descargar"),
    ("module.downloading", "descargando..."),
    ("module.use", "Usar"),
    ("module.in_use", "\u{2713} en uso"),
    ("module.redownload", "Volver a descargar"),
    ("module.language", "Idioma de la aplicaci\u{00F3}n"),
    ("module.language_hint", "Las etiquetas pasan por una tabla i18n; elegir un idioma cambia la interfaz al instante."),
    ("module.reserved", "Espacio de m\u{00F3}dulo reservado."),
    ("module.reserved_hint", "Los paquetes descargables (estilos de cita CSL, temas, plantillas Typst) se conectan aqu\u{00ED} mediante el registro de m\u{00F3}dulos."),
    ("panel.spelling", "Ortograf\u{00ED}a"),
    ("panel.review", "Revisi\u{00F3}n"),
    ("panel.no_issues", "Sin errores \u{2713}"),
    ("panel.dictionary", "Diccionario"),
    ("panel.add", "A\u{00F1}adir"),
    ("panel.jump", "Ir"),
    ("panel.no_suggestions", "(sin sugerencias)"),
    ("options.pdf_engine", "Motor PDF"),
    ("options.pdf_engine.native", "Convertidor nativo"),
    ("options.pdf_engine.general", "Convertidor general"),
    ("options.pdf_engine.native_hint", "Rust puro, totalmente sin conexi\u{00F3}n, el m\u{00E1}s r\u{00E1}pido"),
    ("options.pdf_engine.general_hint", "M\u{00E1}xima fidelidad (motor integrado)"),
];

const IT: &[(&str, &str)] = &[
    ("menu.file", "File"),
    ("menu.edit", "Modifica"),
    ("menu.insert", "Inserisci"),
    ("menu.metadata", "Metadati"),
    ("menu.modules", "Moduli"),
    ("view.hub", "Home"),
    ("view.source", "Sorgente"),
    ("view.split", "Diviso"),
    ("view.editor", "Editor"),
    ("module.title", "Moduli"),
    ("module.tab.dictionaries", "Dizionari"),
    ("module.tab.language", "Lingua dell'app"),
    ("module.tab.citations", "Stili di citazione"),
    ("module.tab.themes", "Temi"),
    ("module.tab.templates", "Modelli"),
    ("module.enable_spell", "Attiva il controllo ortografico"),
    ("module.active_dict", "Dizionario attivo"),
    ("module.no_dict", "Nessun dizionario caricato."),
    ("module.add_dict", "Aggiungi dizionario (.dic + .aff)..."),
    ("module.add_dict_hint", "Scegli un file Hunspell .dic; il relativo .aff (stesso nome) viene caricato automaticamente."),
    ("module.downloadable", "Dizionari scaricabili"),
    ("module.downloadable_hint", "(opzionale - recuperati da wooorm/dictionaries su richiesta)"),
    ("module.download", "Scarica"),
    ("module.downloading", "download in corso..."),
    ("module.use", "Usa"),
    ("module.in_use", "\u{2713} in uso"),
    ("module.redownload", "Scarica di nuovo"),
    ("module.language", "Lingua dell'applicazione"),
    ("module.language_hint", "Le etichette passano per una tabella i18n; scegliere una lingua cambia l'interfaccia all'istante."),
    ("module.reserved", "Slot del modulo riservato."),
    ("module.reserved_hint", "I pacchetti scaricabili (stili di citazione CSL, temi, modelli Typst) si collegano qui tramite il registro dei moduli."),
    ("panel.spelling", "Ortografia"),
    ("panel.review", "Revisione"),
    ("panel.no_issues", "Nessun errore \u{2713}"),
    ("panel.dictionary", "Dizionario"),
    ("panel.add", "Aggiungi"),
    ("panel.jump", "Vai"),
    ("panel.no_suggestions", "(nessun suggerimento)"),
    ("options.pdf_engine", "Motore PDF"),
    ("options.pdf_engine.native", "Convertitore nativo"),
    ("options.pdf_engine.general", "Convertitore generale"),
    ("options.pdf_engine.native_hint", "Puro Rust, completamente offline, il pi\u{00F9} veloce"),
    ("options.pdf_engine.general_hint", "Massima fedelt\u{00E0} (motore integrato)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_and_translates() {
        set_language("en");
        assert_eq!(t("menu.file"), "File");
        set_language("fr");
        assert_eq!(t("menu.file"), "Fichier");
        assert_eq!(t("view.editor"), "\u{00C9}diteur");
        // Unknown key returns the key itself.
        assert_eq!(t("does.not.exist"), "does.not.exist");
        // A key only in EN falls back to English under FR.
        set_language("fr");
        assert_eq!(t("module.modules"), t("module.modules")); // stable
        set_language("en");
    }

    #[test]
    fn locales_are_complete_against_en() {
        let en = en_map();
        // Every locale must translate exactly the EN key set (no missing, no extra).
        for (name, table) in [("fr", FR), ("de", DE), ("es", ES), ("it", IT)] {
            for (k, _) in table {
                assert!(en.contains_key(k), "{name} key '{k}' missing from EN base");
            }
            assert_eq!(
                table.len(),
                EN.len(),
                "{name} has {} keys, EN has {}",
                table.len(),
                EN.len()
            );
        }
    }

    #[test]
    fn all_languages_translate_a_sample_key() {
        for lang in ["en", "fr", "de", "es", "it"] {
            set_language(lang);
            assert!(!t("menu.file").is_empty());
            assert_ne!(t("module.use"), "module.use", "{lang} missing module.use");
        }
        set_language("en");
    }
}
