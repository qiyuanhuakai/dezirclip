use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};
use unicode_general_category::{get_general_category, GeneralCategory};

#[derive(Deserialize)]
struct AnnotationsFile {
    #[serde(alias = "annotationsDerived")]
    annotations: AnnotationMap,
}

#[derive(Deserialize)]
struct AnnotationMap {
    annotations: BTreeMap<String, Annotation>,
}

#[derive(Deserialize)]
struct Annotation {
    #[serde(default)]
    default: Vec<String>,
    #[serde(default)]
    tts: Vec<String>,
}

#[derive(Serialize)]
struct Entry {
    value: String,
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    keywords: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
}

#[derive(Serialize)]
struct Group {
    id: &'static str,
    entries: Vec<Entry>,
}

#[derive(Serialize)]
struct Catalog {
    emoji: Vec<Group>,
    symbols: Vec<Group>,
}

fn load_annotations(
    path: &Path,
) -> Result<BTreeMap<String, Annotation>, Box<dyn std::error::Error>> {
    let input: AnnotationsFile = serde_json::from_reader(File::open(path)?)?;
    Ok(input.annotations.annotations)
}

fn add_keyword(entry: &mut Entry, keyword: &str) {
    if keyword != entry.name && !entry.keywords.iter().any(|word| word == keyword) {
        entry.keywords.push(keyword.to_owned());
    }
}

fn add_arrow_terms(entry: &mut Entry) {
    let words: Vec<_> = entry.name.split_ascii_whitespace().collect();
    let direction = [
        ("LEFTWARDS", "左"),
        ("RIGHTWARDS", "右"),
        ("UPWARDS", "上"),
        ("DOWNWARDS", "下"),
    ]
    .into_iter()
    .filter(|(word, _)| words.contains(word))
    .map(|(_, direction)| direction)
    .collect::<Vec<_>>();
    if direction.len() != 1 {
        return;
    }
    let direction = direction[0];
    let (simple, traditional) = if words.contains(&"DASHED") {
        ("虚线", "虛線")
    } else if words.contains(&"DOUBLE") {
        ("双线", "雙線")
    } else {
        ("", "")
    };
    for (shape, arrow) in [(simple, "箭头"), (traditional, "箭頭")] {
        add_keyword(entry, &format!("向{direction}{shape}{arrow}"));
        add_keyword(entry, &format!("{direction}{shape}{arrow}"));
        if !shape.is_empty() {
            add_keyword(entry, &format!("向{direction}的{shape}{arrow}"));
        }
    }
}

fn add_annotations(entry: &mut Entry, locales: &[BTreeMap<String, Annotation>]) {
    let unqualified = entry.value.replace('\u{fe0f}', "");
    for locale in locales {
        if let Some(annotation) = locale
            .get(&entry.value)
            .or_else(|| locale.get(&unqualified))
        {
            for keyword in annotation.tts.iter().chain(&annotation.default) {
                add_keyword(entry, keyword);
            }
        }
    }
}

fn add_flag_search_terms(entry: &mut Entry) {
    for keyword in entry.keywords.clone() {
        if let Some(region) = keyword.strip_prefix("旗: ") {
            add_keyword(entry, region);
            add_keyword(entry, &format!("{region}国旗"));
            add_keyword(entry, &format!("{region}旗帜"));
        } else if let Some(region) = keyword.strip_prefix("旗子：") {
            add_keyword(entry, region);
            add_keyword(entry, &format!("{region}國旗"));
            add_keyword(entry, &format!("{region}旗幟"));
        } else if let Some(region) = keyword.strip_prefix("flag: ") {
            add_keyword(entry, &format!("{region} flag"));
        }
    }
}

fn emoji_groups(locales: &[BTreeMap<String, Annotation>]) -> Vec<Group> {
    use emojis::Group::*;
    let groups = [
        ("expressions", SmileysAndEmotion),
        ("people", PeopleAndBody),
        ("animals", AnimalsAndNature),
        ("food", FoodAndDrink),
        ("travel", TravelAndPlaces),
        ("activities", Activities),
        ("objects", Objects),
        ("emojiSymbols", Symbols),
        ("flags", Flags),
    ];
    groups
        .into_iter()
        .map(|(id, group)| {
            let entries = group
                .emojis()
                .map(|emoji| {
                    let mut entry = Entry {
                        value: emoji.as_str().to_owned(),
                        name: emoji.name().to_owned(),
                        keywords: emoji.shortcodes().map(str::to_owned).collect(),
                        aliases: Vec::new(),
                    };
                    add_annotations(&mut entry, locales);
                    if group == Flags {
                        add_flag_search_terms(&mut entry);
                    }
                    entry
                })
                .collect();
            Group { id, entries }
        })
        .collect()
}

fn symbol_group(character: char) -> Option<usize> {
    use GeneralCategory::*;
    let point = character as u32;
    if (0x2190..=0x21ff).contains(&point)
        || (0x27f0..=0x27ff).contains(&point)
        || (0x2900..=0x297f).contains(&point)
        || (0x1f800..=0x1f8ff).contains(&point)
    {
        return Some(0);
    }
    if (0x391..=0x3a9).contains(&point) || (0x3b1..=0x3c9).contains(&point) {
        return Some(1);
    }
    if (0x2460..=0x249b).contains(&point) && get_general_category(character) == OtherNumber {
        return Some(4);
    }
    if (0x2500..=0x259f).contains(&point) {
        return Some(2);
    }
    match get_general_category(character) {
        MathSymbol => Some(1),
        CurrencySymbol => Some(3),
        OtherSymbol | ModifierSymbol => Some(4),
        DashPunctuation | OpenPunctuation | ClosePunctuation | InitialPunctuation
        | FinalPunctuation | OtherPunctuation
            if character.is_ascii()
                || (0x00a1..=0x00ff).contains(&point)
                || (0x2000..=0x206f).contains(&point)
                || (0x3000..=0x303f).contains(&point) =>
        {
            Some(5)
        }
        _ => None,
    }
}

fn symbol_groups(locales: &[BTreeMap<String, Annotation>]) -> Vec<Group> {
    let mut groups: Vec<_> = [
        "arrows",
        "math",
        "boxDrawing",
        "currency",
        "otherSymbols",
        "punctuation",
    ]
    .into_iter()
    .map(|id| Group {
        id,
        entries: Vec::new(),
    })
    .collect();
    for character in (0..=0x10ffff).filter_map(char::from_u32) {
        let Some(group) = symbol_group(character) else {
            continue;
        };
        let Some(name) = unicode_names2::name(character) else {
            continue;
        };
        let value = character.to_string();
        if emojis::get(&value).is_some_and(|emoji| emoji.as_str() == value) {
            continue;
        }
        let mut entry = Entry {
            value,
            name: name.to_string(),
            keywords: Vec::new(),
            aliases: Vec::new(),
        };
        add_annotations(&mut entry, locales);
        if group == 0 {
            add_arrow_terms(&mut entry);
        }
        groups[group].entries.push(entry);
    }
    groups
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let (output, annotations) = args
        .split_last()
        .filter(|(_, annotations)| annotations.len() == 6)
        .ok_or("pass the three base and three derived annotation files followed by output.json")?;
    let locales = annotations
        .iter()
        .map(|path| load_annotations(Path::new(path)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut catalog = Catalog {
        emoji: emoji_groups(&locales),
        symbols: symbol_groups(&locales),
    };
    let aliases: BTreeMap<String, Vec<String>> =
        serde_json::from_str(include_str!("../aliases.json"))?;
    for entry in catalog
        .emoji
        .iter_mut()
        .chain(&mut catalog.symbols)
        .flat_map(|group| &mut group.entries)
    {
        if let Some(words) = aliases.get(&entry.value) {
            entry.aliases.clone_from(words);
        }
    }
    let mut output = BufWriter::new(File::create(output)?);
    serde_json::to_writer(&mut output, &catalog)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
