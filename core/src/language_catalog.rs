// ISO 639-1 plus additional commonly learned languages.
// Codes and aliases: https://www.loc.gov/standards/iso639-2/ISO-639-2_utf-8.txt
// Display names captured from Unicode CLDR through Intl.DisplayNames.
// Unicode data license: notices/Unicode-3.0.txt.
use super::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageOption {
    code: LanguageId,
    name: &'static str,
    native_name: &'static str,
    #[serde(skip_serializing)]
    aliases: &'static [&'static str],
}

// Define the enum and its display metadata together, with one source of truth.
macro_rules! languages {
    ($($id:ident => ($code:literal, $name:literal, $native:literal, [$($alias:literal),*])),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize)]
        enum LanguageId { $(#[serde(rename = $code)] $id),* }
        impl LanguageId {
            fn as_str(self) -> &'static str { match self { $(Self::$id => $code),* } }
        }
        const LANGUAGES: &[LanguageOption] = &[$(LanguageOption {
            code: LanguageId::$id, name: $name, native_name: $native, aliases: &[$($alias),*],
        }),*];
    };
}

languages! {
    Ab => ("ab", "Abkhazian", "", []),
    Aa => ("aa", "Afar", "", []),
    Af => ("af", "Afrikaans", "", []),
    Ak => ("ak", "Akan", "", []),
    Sq => ("sq", "Albanian", "shqip", []),
    Am => ("am", "Amharic", "አማርኛ", []),
    Ar => ("ar", "Arabic", "العربية", []),
    An => ("an", "Aragonese", "", []),
    Hy => ("hy", "Armenian", "հայերեն", []),
    As => ("as", "Assamese", "অসমীয়া", []),
    Ast => ("ast", "Asturian", "asturianu", ["Bable", "Leonese", "Asturleonese"]),
    Av => ("av", "Avaric", "", []),
    Ae => ("ae", "Avestan", "", []),
    Ay => ("ay", "Aymara", "", []),
    Az => ("az", "Azerbaijani", "azərbaycan", []),
    Bm => ("bm", "Bambara", "bamanakan", []),
    Bn => ("bn", "Bangla", "বাংলা", ["Bengali"]),
    Ba => ("ba", "Bashkir", "башҡорт", []),
    Eu => ("eu", "Basque", "euskara", []),
    Be => ("be", "Belarusian", "беларуская", []),
    Bho => ("bho", "Bhojpuri", "भोजपुरी", []),
    Bi => ("bi", "Bislama", "", []),
    Bs => ("bs", "Bosnian", "bosanski", []),
    Br => ("br", "Breton", "brezhoneg", []),
    Bg => ("bg", "Bulgarian", "български", []),
    My => ("my", "Burmese", "မြန်မာ", []),
    Yue => ("yue", "Cantonese", "粵語", ["Yue", "广东话", "廣東話", "粤语"]),
    Ca => ("ca", "Catalan", "català", ["Valencian"]),
    Ceb => ("ceb", "Cebuano", "", []),
    Ch => ("ch", "Chamorro", "", []),
    Ce => ("ce", "Chechen", "нохчийн", []),
    Zh => ("zh", "Chinese (Mandarin)", "中文", ["Chinese", "Mandarin", "Mandarin Chinese", "普通话", "普通話"]),
    Cu => ("cu", "Church Slavic", "", ["Old Slavonic", "Church Slavonic", "Old Bulgarian", "Old Church Slavonic"]),
    Cv => ("cv", "Chuvash", "чӑваш чӗлхи", []),
    Kw => ("kw", "Cornish", "kernewek", []),
    Co => ("co", "Corsican", "", []),
    Cr => ("cr", "Cree", "", []),
    Hr => ("hr", "Croatian", "hrvatski", []),
    Cs => ("cs", "Czech", "čeština", []),
    Da => ("da", "Danish", "dansk", []),
    Dv => ("dv", "Divehi", "", ["Dhivehi", "Maldivian"]),
    Nl => ("nl", "Dutch", "Nederlands", ["Flemish"]),
    Dz => ("dz", "Dzongkha", "རྫོང་ཁ", []),
    En => ("en", "English", "", []),
    Eo => ("eo", "Esperanto", "", []),
    Et => ("et", "Estonian", "eesti", []),
    Ee => ("ee", "Ewe", "eʋegbe", []),
    Fo => ("fo", "Faroese", "føroyskt", []),
    Fj => ("fj", "Fijian", "", []),
    Fil => ("fil", "Filipino", "", ["Pilipino"]),
    Fi => ("fi", "Finnish", "suomi", []),
    Fr => ("fr", "French", "français", []),
    Ff => ("ff", "Fula", "Pulaar", ["Fulah"]),
    Gl => ("gl", "Galician", "galego", []),
    Lg => ("lg", "Ganda", "Luganda", []),
    Ka => ("ka", "Georgian", "ქართული", []),
    De => ("de", "German", "Deutsch", []),
    El => ("el", "Greek", "Ελληνικά", ["Modern Greek (1453-)"]),
    Gn => ("gn", "Guarani", "", []),
    Gu => ("gu", "Gujarati", "ગુજરાતી", []),
    Ht => ("ht", "Haitian Creole", "", ["Haitian"]),
    Ha => ("ha", "Hausa", "", []),
    Haw => ("haw", "Hawaiian", "ʻŌlelo Hawaiʻi", []),
    He => ("he", "Hebrew", "עברית", []),
    Hz => ("hz", "Herero", "", []),
    Hi => ("hi", "Hindi", "हिन्दी", []),
    Ho => ("ho", "Hiri Motu", "", []),
    Hmn => ("hmn", "Hmong", "", ["Mong"]),
    Hu => ("hu", "Hungarian", "magyar", []),
    Is => ("is", "Icelandic", "íslenska", []),
    Io => ("io", "Ido", "", []),
    Ig => ("ig", "Igbo", "", []),
    Ilo => ("ilo", "Iloko", "", []),
    Id => ("id", "Indonesian", "Indonesia", []),
    Ia => ("ia", "Interlingua", "interlingua", ["Interlingua (International Auxiliary Language Association)"]),
    Ie => ("ie", "Interlingue", "", ["Occidental"]),
    Iu => ("iu", "Inuktitut", "", []),
    Ik => ("ik", "Inupiaq", "", []),
    Ga => ("ga", "Irish", "Gaeilge", []),
    It => ("it", "Italian", "italiano", []),
    Ja => ("ja", "Japanese", "日本語", []),
    Jv => ("jv", "Javanese", "Jawa", []),
    Kl => ("kl", "Kalaallisut", "kalaallisut", ["Greenlandic"]),
    Kn => ("kn", "Kannada", "ಕನ್ನಡ", []),
    Kr => ("kr", "Kanuri", "", []),
    Ks => ("ks", "Kashmiri", "کٲشُر", []),
    Kk => ("kk", "Kazakh", "қазақ тілі", []),
    Km => ("km", "Khmer", "ខ្មែរ", ["Central Khmer"]),
    Ki => ("ki", "Kikuyu", "Gikuyu", ["Gikuyu"]),
    Rw => ("rw", "Kinyarwanda", "Ikinyarwanda", []),
    Kv => ("kv", "Komi", "", []),
    Kg => ("kg", "Kongo", "", []),
    Kok => ("kok", "Konkani", "कोंकणी", []),
    Ko => ("ko", "Korean", "한국어", []),
    Kj => ("kj", "Kuanyama", "", ["Kwanyama"]),
    Ku => ("ku", "Kurdish", "kurdî (kurmancî)", []),
    Ky => ("ky", "Kyrgyz", "кыргызча", ["Kirghiz"]),
    Lo => ("lo", "Lao", "ລາວ", []),
    La => ("la", "Latin", "", []),
    Lv => ("lv", "Latvian", "latviešu", []),
    Li => ("li", "Limburgish", "", ["Limburgan", "Limburger"]),
    Ln => ("ln", "Lingala", "lingála", []),
    Lt => ("lt", "Lithuanian", "lietuvių", []),
    Lu => ("lu", "Luba-Katanga", "Tshiluba", []),
    Lb => ("lb", "Luxembourgish", "Lëtzebuergesch", ["Letzeburgesch"]),
    Mk => ("mk", "Macedonian", "македонски", []),
    Mai => ("mai", "Maithili", "मैथिली", []),
    Mg => ("mg", "Malagasy", "", []),
    Ms => ("ms", "Malay", "Melayu", []),
    Ml => ("ml", "Malayalam", "മലയാളം", []),
    Mt => ("mt", "Maltese", "Malti", []),
    Mni => ("mni", "Manipuri", "মৈতৈলোন্", []),
    Gv => ("gv", "Manx", "Gaelg", []),
    Mi => ("mi", "Māori", "", ["Maori"]),
    Mr => ("mr", "Marathi", "मराठी", []),
    Mh => ("mh", "Marshallese", "", []),
    Mn => ("mn", "Mongolian", "монгол", []),
    Na => ("na", "Nauru", "", []),
    Nv => ("nv", "Navajo", "", ["Navaho"]),
    Ng => ("ng", "Ndonga", "", []),
    Ne => ("ne", "Nepali", "नेपाली", []),
    Nd => ("nd", "North Ndebele", "isiNdebele", []),
    Se => ("se", "Northern Sami", "davvisámegiella", []),
    No => ("no", "Norwegian", "norsk", []),
    Nb => ("nb", "Norwegian Bokmål", "norsk bokmål", []),
    Nn => ("nn", "Norwegian Nynorsk", "norsk nynorsk", []),
    Ny => ("ny", "Nyanja", "", ["Chichewa", "Chewa"]),
    Oc => ("oc", "Occitan", "occitan", ["Occitan (post 1500)"]),
    Or => ("or", "Odia", "ଓଡ଼ିଆ", ["Oriya"]),
    Oj => ("oj", "Ojibwa", "", []),
    Om => ("om", "Oromo", "Oromoo", []),
    Os => ("os", "Ossetic", "ирон", ["Ossetian"]),
    Pi => ("pi", "Pali", "", []),
    Ps => ("ps", "Pashto", "پښتو", ["Pushto"]),
    Fa => ("fa", "Persian", "فارسی", ["Farsi"]),
    Pl => ("pl", "Polish", "polski", []),
    Pt => ("pt", "Portuguese", "português", []),
    Pa => ("pa", "Punjabi", "ਪੰਜਾਬੀ", ["Panjabi"]),
    Qu => ("qu", "Quechua", "Runasimi", []),
    Ro => ("ro", "Romanian", "română", ["Moldavian", "Moldovan"]),
    Rm => ("rm", "Romansh", "rumantsch", []),
    Rn => ("rn", "Rundi", "Ikirundi", []),
    Ru => ("ru", "Russian", "русский", []),
    Sm => ("sm", "Samoan", "", []),
    Sg => ("sg", "Sango", "Sängö", []),
    Sa => ("sa", "Sanskrit", "संस्कृत भाषा", []),
    Sat => ("sat", "Santali", "ᱥᱟᱱᱛᱟᱲᱤ", []),
    Sc => ("sc", "Sardinian", "sardu", []),
    Sco => ("sco", "Scots", "", []),
    Gd => ("gd", "Scottish Gaelic", "Gàidhlig", ["Gaelic"]),
    Sr => ("sr", "Serbian", "српски", []),
    Sn => ("sn", "Shona", "chiShona", []),
    Ii => ("ii", "Sichuan Yi", "ꆈꌠꉙ", ["Nuosu"]),
    Sd => ("sd", "Sindhi", "سنڌي", []),
    Si => ("si", "Sinhala", "සිංහල", ["Sinhalese"]),
    Sk => ("sk", "Slovak", "slovenčina", []),
    Sl => ("sl", "Slovenian", "slovenščina", []),
    So => ("so", "Somali", "Soomaali", []),
    Nr => ("nr", "South Ndebele", "", []),
    St => ("st", "Southern Sotho", "Sesotho", ["Sotho, Southern"]),
    Es => ("es", "Spanish", "español", ["Castilian"]),
    Su => ("su", "Sundanese", "Basa Sunda", []),
    Sw => ("sw", "Swahili", "Kiswahili", []),
    Ss => ("ss", "Swati", "", []),
    Sv => ("sv", "Swedish", "svenska", []),
    Tl => ("tl", "Tagalog", "", ["Filipino"]),
    Ty => ("ty", "Tahitian", "", []),
    Tg => ("tg", "Tajik", "тоҷикӣ", []),
    Ta => ("ta", "Tamil", "தமிழ்", []),
    Tt => ("tt", "Tatar", "татар", []),
    Te => ("te", "Telugu", "తెలుగు", []),
    Th => ("th", "Thai", "ไทย", []),
    Bo => ("bo", "Tibetan", "བོད་སྐད་", []),
    Ti => ("ti", "Tigrinya", "ትግርኛ", []),
    To => ("to", "Tongan", "lea fakatonga", ["Tonga (Tonga Islands)"]),
    Ts => ("ts", "Tsonga", "", []),
    Tn => ("tn", "Tswana", "Setswana", []),
    Tr => ("tr", "Turkish", "Türkçe", []),
    Tk => ("tk", "Turkmen", "türkmen dili", []),
    Tw => ("tw", "Twi", "", ["Akan"]),
    Uk => ("uk", "Ukrainian", "українська", []),
    Ur => ("ur", "Urdu", "اردو", []),
    Ug => ("ug", "Uyghur", "ئۇيغۇرچە", ["Uighur"]),
    Uz => ("uz", "Uzbek", "o‘zbek", []),
    Ve => ("ve", "Venda", "", []),
    Vi => ("vi", "Vietnamese", "Tiếng Việt", []),
    Vo => ("vo", "Volapük", "", []),
    Wa => ("wa", "Walloon", "", []),
    Cy => ("cy", "Welsh", "Cymraeg", []),
    Fy => ("fy", "Western Frisian", "Frysk", []),
    Wo => ("wo", "Wolof", "", []),
    Xh => ("xh", "Xhosa", "IsiXhosa", []),
    Yi => ("yi", "Yiddish", "ייִדיש", []),
    Yo => ("yo", "Yoruba", "Èdè Yorùbá", []),
    Za => ("za", "Zhuang", "Vahcuengh", ["Chuang"]),
    Zu => ("zu", "Zulu", "isiZulu", []),
}

fn folded(value: &str) -> String {
    value
        .nfkd()
        .filter(|c| !matches!(c, '\u{0300}'..='\u{036f}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn resolve(value: &str) -> Option<&'static LanguageOption> {
    let key = folded(value);
    if key.is_empty() {
        return None;
    }
    // Prefer canonical names/codes over aliases of related languages.
    LANGUAGES
        .iter()
        .find(|option| {
            [option.code.as_str(), option.name, option.native_name]
                .iter()
                .any(|name| !name.is_empty() && folded(name) == key)
        })
        .or_else(|| {
            LANGUAGES
                .iter()
                .find(|option| option.aliases.iter().any(|name| folded(name) == key))
        })
}

pub(super) fn translation_language(value: &str) -> Option<(&'static str, &'static str)> {
    resolve(value)
        .or_else(|| resolve(value.split(['-', '_']).next()?))
        .map(|language| (language.code.as_str(), language.name))
}

#[derive(Default, Deserialize)]
struct ChoicesRequest {
    #[serde(default)]
    query: String,
    #[serde(default)]
    excluded: Vec<String>,
    #[serde(default)]
    selected: String,
}

pub(super) fn choices(command: &Value) -> Result<Value> {
    let request: ChoicesRequest =
        serde_json::from_value(command.clone()).map_err(|_| "Invalid language picker request.")?;
    if request.query.chars().count() > 200
        || request.selected.chars().count() > 80
        || request.excluded.len() > 50
        || request
            .excluded
            .iter()
            .any(|name| name.chars().count() > 80)
    {
        return Err("Invalid language picker request.".into());
    }
    let key = folded(&request.query);
    let excluded: HashSet<_> = request
        .excluded
        .iter()
        .filter_map(|name| resolve(name).map(|option| option.code))
        .collect();
    let options: Vec<_> = LANGUAGES
        .iter()
        .filter(|option| {
            !excluded.contains(&option.code)
                && [option.code.as_str(), option.name, option.native_name]
                    .into_iter()
                    .chain(option.aliases.iter().copied())
                    .any(|name| folded(name).contains(&key))
        })
        .collect();
    let sections = if key.is_empty() {
        use LanguageId::{Ar, De, En, Es, Fr, Hi, It, Ja, Ko, Pt, Ru, Zh};
        let common = [En, Es, Fr, De, Zh, Ja, Ko, It, Pt, Ar, Hi, Ru];
        let popular: Vec<_> = common
            .iter()
            .filter_map(|code| options.iter().find(|option| option.code == *code).copied())
            .collect();
        let other: Vec<_> = options
            .into_iter()
            .filter(|option| !common.contains(&option.code))
            .collect();
        vec![
            json!({"title":"Common languages", "data":popular}),
            json!({"title":"More languages", "data":other}),
        ]
    } else {
        vec![json!({"title":"Languages", "data":options})]
    };
    Ok(
        json!({"selected":resolve(&request.selected), "sections":sections.into_iter().filter(|section| section["data"].as_array().is_some_and(|data| !data.is_empty())).collect::<Vec<_>>()}),
    )
}
