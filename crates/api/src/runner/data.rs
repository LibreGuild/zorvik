//! Data files for collection runs: CSV with a header row, or a JSON array of
//! objects. One row per iteration; its columns are variables (`{{name}}`,
//! `pm.iterationData`). CSV values are text; JSON values keep their type
//! (non-strings become JSON text as variables).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

/// Data files larger than this are refused.
pub const MAX_DATA_FILE: u64 = 50 * 1024 * 1024;
/// Rows at most: one per iteration (more could never run; a big file of short
/// rows would otherwise take gigabytes of memory).
pub const MAX_ROWS: usize = super::MAX_ITERATIONS as usize;
/// Rows the preview shows.
const PREVIEW_ROWS: usize = 5;
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// One iteration's values.
pub type DataRow = BTreeMap<String, Value>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DataFile {
    /// Column names in file order (JSON: keys in order of first appearance).
    pub columns: Vec<String>,
    pub rows: Vec<DataRow>,
}

/// The start of a data file, for the runner's settings (`runner.preview`).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DataPreview {
    /// `csv` or `json`.
    pub format: String,
    pub columns: Vec<String>,
    /// The first rows, values as variables see them, in column order.
    pub rows: Vec<Vec<String>>,
    /// All rows.
    pub count: u32,
}

impl DataFile {
    pub fn preview(&self, format: &str) -> DataPreview {
        let rows = self
            .rows
            .iter()
            .take(PREVIEW_ROWS)
            .map(|row| {
                self.columns.iter().map(|c| row.get(c).map(zorvik_script::value_text).unwrap_or_default()).collect()
            })
            .collect();
        DataPreview { format: format.into(), columns: self.columns.clone(), rows, count: self.rows.len() as u32 }
    }
}

/// `json` for `.json` files and content that starts with `[`, else `csv`.
pub fn data_format(name: &str, bytes: &[u8]) -> &'static str {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let bytes = bytes.strip_prefix(BOM).unwrap_or(bytes);
    match ext.as_str() {
        "json" => "json",
        "csv" => "csv",
        _ if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[') => "json",
        _ => "csv",
    }
}

/// Parse a data file; `name` (the file name) picks the format by extension.
pub fn parse_data(name: &str, bytes: &[u8]) -> Result<DataFile, String> {
    let format = data_format(name, bytes);
    let bytes = bytes.strip_prefix(BOM).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|e| {
        let line = bytes[..e.valid_up_to()].iter().filter(|b| **b == b'\n').count() + 1;
        format!("The data file is not UTF-8 text (line {line}). Save it as UTF-8, e.g. \"CSV UTF-8\" in Excel.")
    })?;
    let data = match format {
        "json" => parse_json(text)?,
        _ => parse_csv(text)?,
    };
    if data.rows.is_empty() {
        return Err("The data file has no rows".into());
    }
    Ok(data)
}

fn parse_json(text: &str) -> Result<DataFile, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("The data file is not valid JSON: {e}"))?;
    let Value::Array(items) = value else {
        return Err("JSON data must be an array of objects, one per iteration".into());
    };
    if items.len() > MAX_ROWS {
        return Err(too_many_rows());
    }
    let mut data = DataFile::default();
    for (i, item) in items.into_iter().enumerate() {
        let Value::Object(map) = item else {
            return Err(format!("Item {} of the JSON data is not an object", i + 1));
        };
        for key in map.keys() {
            if !data.columns.contains(key) {
                data.columns.push(key.clone());
            }
        }
        data.rows.push(map.into_iter().collect());
    }
    Ok(data)
}

fn parse_csv(text: &str) -> Result<DataFile, String> {
    let mut records = csv_records(text)?.into_iter();
    let Some((_, header)) = records.next() else { return Ok(DataFile::default()) };
    let columns: Vec<String> = header.iter().map(|c| c.trim().to_string()).collect();
    for (i, column) in columns.iter().enumerate() {
        if column.is_empty() {
            return Err(format!("Column {} of the CSV header has no name", i + 1));
        }
        if columns[..i].contains(column) {
            return Err(format!("The CSV header has the column '{column}' twice"));
        }
    }
    let mut rows = Vec::new();
    for (line, mut fields) in records {
        // Trailing empty values (a comma at the end of each line) are harmless.
        while fields.len() > columns.len() && fields.last().is_some_and(String::is_empty) {
            fields.pop();
        }
        if fields.len() > columns.len() {
            return Err(format!(
                "Line {line} of the CSV data has {} values, but the header has {} columns",
                fields.len(),
                columns.len()
            ));
        }
        // Missing values at the end are empty.
        fields.resize(columns.len(), String::new());
        rows.push(columns.iter().cloned().zip(fields.into_iter().map(Value::String)).collect());
    }
    Ok(DataFile { columns, rows })
}

/// CSV records (RFC 4180): comma-separated, `"quoted"` fields may hold commas,
/// line breaks and `""` (a quote); CRLF or LF line ends; blank lines skipped.
/// Each record comes with the line it starts on.
fn csv_records(text: &str) -> Result<Vec<(usize, Vec<String>)>, String> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    // The current field started with a quote (a blank line is only one without any).
    let mut quoted = false;
    let mut in_quotes = false;
    let mut line = 1;
    let mut record_line = 1;
    let mut quote_line = 1;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !quoted => {
                quoted = true;
                in_quotes = true;
                quote_line = line;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                quoted = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                let blank = record.is_empty() && field.is_empty() && !quoted;
                if !blank {
                    record.push(std::mem::take(&mut field));
                    records.push((record_line, std::mem::take(&mut record)));
                    // The header and one row per iteration at most.
                    if records.len() > MAX_ROWS + 1 {
                        return Err(too_many_rows());
                    }
                }
                quoted = false;
                line += 1;
                record_line = line;
            }
            _ => field.push(c),
        }
    }
    if in_quotes {
        return Err(format!("The CSV data has a quote on line {quote_line} that is never closed"));
    }
    if !record.is_empty() || !field.is_empty() || quoted {
        record.push(field);
        records.push((record_line, record));
    }
    if records.len() > MAX_ROWS + 1 {
        return Err(too_many_rows());
    }
    Ok(records)
}

fn too_many_rows() -> String {
    format!("The data file has more than {MAX_ROWS} rows (the most iterations a run can have)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows(data: &DataFile) -> Vec<Vec<(String, Value)>> {
        data.rows.iter().map(|r| r.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).collect()
    }

    fn row(pairs: &[(&str, Value)]) -> Vec<(String, Value)> {
        let mut out: Vec<(String, Value)> = pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    #[test]
    fn csv_with_quotes_line_breaks_bom_and_crlf() {
        let text =
            "\u{feff}id, name ,note\r\n1,Ada,\"likes \"\"maths\"\", logic\"\r\n\r\n2,\"Grace\nHopper\",\r\n3,Linus";
        let data = parse_data("users.csv", text.as_bytes()).unwrap();
        assert_eq!(data.columns, ["id", "name", "note"]);
        assert_eq!(
            rows(&data),
            [
                row(&[("id", json!("1")), ("name", json!("Ada")), ("note", json!("likes \"maths\", logic"))]),
                row(&[("id", json!("2")), ("name", json!("Grace\nHopper")), ("note", json!(""))]),
                // A missing value at the end is empty.
                row(&[("id", json!("3")), ("name", json!("Linus")), ("note", json!(""))]),
            ]
        );
        // Values are kept as they are (spaces too); a trailing comma is fine.
        let data = parse_data("x.csv", b"a,b\n x ,\"\",\n").unwrap();
        assert_eq!(rows(&data), [row(&[("a", json!(" x ")), ("b", json!(""))])]);
        // Quotes inside an unquoted value are literal.
        let data = parse_data("x.csv", b"a\n5\"x\n").unwrap();
        assert_eq!(rows(&data), [row(&[("a", json!("5\"x"))])]);
        // An empty quoted line is a row (one empty value), unlike a blank line.
        assert_eq!(parse_data("x.csv", b"a\n\"\"\n\n").unwrap().rows.len(), 1);
    }

    #[test]
    fn csv_errors() {
        let err = |text: &str| parse_data("d.csv", text.as_bytes()).unwrap_err();
        assert_eq!(err("a,b\n1,\"open\n2,3\n"), "The CSV data has a quote on line 2 that is never closed");
        assert_eq!(err("a,b\n1,2\n1,2,3\n"), "Line 3 of the CSV data has 3 values, but the header has 2 columns");
        assert_eq!(err("a,,c\n1,2,3"), "Column 2 of the CSV header has no name");
        assert_eq!(err("a,a\n1,2"), "The CSV header has the column 'a' twice");
        assert_eq!(err("a,b\n"), "The data file has no rows");
        assert_eq!(err(""), "The data file has no rows");
        assert_eq!(
            parse_data("d.csv", b"a\nx\n\xe9t\xe9\n").unwrap_err(),
            "The data file is not UTF-8 text (line 3). Save it as UTF-8, e.g. \"CSV UTF-8\" in Excel."
        );
    }

    #[test]
    fn rows_are_limited_to_the_most_iterations() {
        let csv = |rows: usize| format!("id\n{}", "1\n".repeat(rows));
        assert_eq!(parse_data("d.csv", csv(MAX_ROWS).as_bytes()).unwrap().rows.len(), MAX_ROWS);
        let too_many = format!("The data file has more than {MAX_ROWS} rows (the most iterations a run can have)");
        assert_eq!(parse_data("d.csv", csv(MAX_ROWS + 1).as_bytes()).unwrap_err(), too_many);
        // Without a line break at the end too.
        assert_eq!(parse_data("d.csv", format!("{}1", csv(MAX_ROWS)).as_bytes()).unwrap_err(), too_many);
        let json = |rows: usize| format!("[{}{{}}]", "{},".repeat(rows - 1));
        assert_eq!(parse_data("d.json", json(MAX_ROWS).as_bytes()).unwrap().rows.len(), MAX_ROWS);
        assert_eq!(parse_data("d.json", json(MAX_ROWS + 1).as_bytes()).unwrap_err(), too_many);
    }

    #[test]
    fn json_rows_keep_types() {
        let text = r#"[{"id": 1, "name": "Ada", "tags": ["a"]}, {"name": "Grace", "admin": true, "none": null}]"#;
        let data = parse_data("users.json", text.as_bytes()).unwrap();
        assert_eq!(data.columns, ["id", "name", "tags", "admin", "none"]);
        assert_eq!(data.rows[0]["id"], json!(1));
        assert_eq!(data.rows[1]["admin"], json!(true));
        let preview = data.preview("json");
        assert_eq!(preview.count, 2);
        assert_eq!(preview.rows, [vec!["1", "Ada", "[\"a\"]", "", ""], vec!["", "Grace", "", "true", ""]]);

        let err = |text: &str| parse_data("d.json", text.as_bytes()).unwrap_err();
        assert_eq!(err(r#"{"a": 1}"#), "JSON data must be an array of objects, one per iteration");
        assert_eq!(err(r#"[{"a": 1}, 2]"#), "Item 2 of the JSON data is not an object");
        assert_eq!(err("[]"), "The data file has no rows");
        assert!(err("[{").starts_with("The data file is not valid JSON"));
    }

    #[test]
    fn format_by_extension_or_content() {
        assert_eq!(data_format("a.JSON", b"id\n1"), "json");
        assert_eq!(data_format("a.csv", b"[1]"), "csv");
        assert_eq!(data_format("data.txt", b"\xEF\xBB\xBF  [{\"a\": 1}]"), "json");
        assert_eq!(data_format("data", b"a,b"), "csv");
        let data = parse_data("rows.txt", br#"[{"a": "x"}]"#).unwrap();
        assert_eq!(data.rows[0]["a"], json!("x"));
    }
}
