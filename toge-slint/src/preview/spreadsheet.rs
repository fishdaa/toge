use super::{
    AtomicU64, Command, DOCUMENT_TIMEOUT, MAX_DOCUMENT_BYTES, MAX_TEXT_BYTES, Ordering, Path,
    Preview, ScratchDir, fs, render_document_page, run_stdout,
};

// Standard-library reader: bounded sheet data without another format crate.
const SCRIPT: &str = include_str!("spreadsheet.py");

pub(super) fn load(path: &Path, request: u64, serial: &AtomicU64) -> Result<Preview, &'static str> {
    if fs::metadata(path).map_err(|_| "File is unavailable")?.len() > MAX_DOCUMENT_BYTES {
        return Err("Spreadsheet too large to preview");
    }
    let scratch = ScratchDir::new().map_err(|_| "Cannot prepare spreadsheet preview")?;
    let output = scratch.join("sheet.txt");
    let mut command = Command::new("python3");
    command.args(["-I", "-c", SCRIPT]).arg(path);
    if run_stdout(
        command,
        &output,
        (MAX_TEXT_BYTES + 256) as u64,
        DOCUMENT_TIMEOUT,
        request,
        serial,
        "Install Python 3 for spreadsheet data previews",
        "Cannot read spreadsheet",
    )
    .is_ok()
    {
        return fs::read_to_string(output)
            .map(Preview::Text)
            .map_err(|_| "Cannot read spreadsheet");
    }
    if serial.load(Ordering::Acquire) != request {
        return Err("Preview cancelled");
    }
    if let Some(pixels) = render_document_page(path, request, serial) {
        return Ok(Preview::Image(pixels));
    }
    Err("Cannot preview spreadsheet; install Python 3 or LibreOffice and Poppler")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::tools;
    fn package(path: &Path, entries: &[(&str, &str)]) {
        let json = serde_json::to_string(entries).unwrap();
        assert!(Command::new("python3").args(["-I", "-c", "import sys,json,zipfile\nwith zipfile.ZipFile(sys.argv[1], 'w') as z:\n for name,data in json.loads(sys.argv[2]): z.writestr(name,data)"])
            .arg(path).arg(json).status().unwrap().success());
    }
    #[test]
    fn xlsx_resolves_sheet_names_strings_sparse_cells_and_cached_formulas() {
        if !tools::available(std::ffi::OsStr::new("python3")) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.xlsx");
        package(
            &path,
            &[
                (
                    "xl/workbook.xml",
                    r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" r:id="sheet1"/></sheets></workbook>"#,
                ),
                (
                    "xl/_rels/workbook.xml.rels",
                    r#"<Relationships><Relationship Id="sheet1" Target="worksheets/sheet1.xml"/></Relationships>"#,
                ),
                (
                    "xl/sharedStrings.xml",
                    r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Team &amp; tools</t></si></sst>"#,
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="4"><c r="A4" t="s"><v>0</v></c><c r="C4" t="inlineStr"><is><t>Inline</t></is></c><c r="D4"><f>2+2</f><v>4</v></c><c r="E4" t="b"><v>1</v></c></row></sheetData></worksheet>"#,
                ),
            ],
        );
        let Preview::Text(text) = load(&path, 1, &AtomicU64::new(1)).unwrap() else {
            panic!("text expected")
        };
        assert!(text.contains("Sheet: Budget"));
        assert!(text.contains("A4: Team & tools | C4: Inline | D4: 4 | E4: TRUE"));
        assert!(text.contains("formula recalculation"));
    }
    #[test]
    fn ods_bounds_repeated_rows_and_columns() {
        if !tools::available(std::ffi::OsStr::new("python3")) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.ods");
        package(
            &path,
            &[(
                "content.xml",
                r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><table:table table:name="Data"><table:table-row table:number-rows-repeated="1000000000"><table:table-cell table:number-columns-repeated="1000000000"><text:p>Bounded</text:p></table:table-cell></table:table-row></table:table></office:document-content>"#,
            )],
        );
        let Preview::Text(text) = load(&path, 1, &AtomicU64::new(1)).unwrap() else {
            panic!("text expected")
        };
        assert!(text.contains("Sheet: Data"));
        assert_eq!(
            text.lines().filter(|line| line.contains("Bounded")).count(),
            200
        );
        assert!(text.len() <= MAX_TEXT_BYTES + 256);
        assert!(load(&path, 1, &AtomicU64::new(2)).is_err());
    }
}
