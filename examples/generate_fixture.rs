//! Regenerate `fixtures/sample.xlsx` (two joinable sheets used by the README example).
//! Run: `cargo run --example generate_fixture`

use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

fn main() {
    let mut wb = Workbook::new();
    let date_fmt = Format::new().set_num_format("yyyy-mm-dd");

    let people = wb.add_worksheet().set_name("People").unwrap();
    for (c, h) in ["name", "age", "active", "joined"].iter().enumerate() {
        people.write_string(0, c as u16, *h).unwrap();
    }
    let rows = [
        ("ada", 36.0, true, "2024-01-15"),
        ("grace", 45.0, false, "2023-11-02"),
        ("linus", 54.0, true, "2024-06-30"),
    ];
    for (r, (name, age, active, joined)) in rows.iter().enumerate() {
        let r = (r + 1) as u32;
        people.write_string(r, 0, *name).unwrap();
        people.write_number(r, 1, *age).unwrap();
        people.write_boolean(r, 2, *active).unwrap();
        let dt = ExcelDateTime::parse_from_str(joined).unwrap();
        people
            .write_datetime_with_format(r, 3, &dt, &date_fmt)
            .unwrap();
    }

    let orders = wb.add_worksheet().set_name("Orders").unwrap();
    for (c, h) in ["name", "amount"].iter().enumerate() {
        orders.write_string(0, c as u16, *h).unwrap();
    }
    for (r, (name, amount)) in [("ada", 120.5), ("ada", 30.0), ("linus", 99.99)]
        .iter()
        .enumerate()
    {
        let r = (r + 1) as u32;
        orders.write_string(r, 0, *name).unwrap();
        orders.write_number(r, 1, *amount).unwrap();
    }

    wb.save("fixtures/sample.xlsx").unwrap();
    eprintln!("wrote fixtures/sample.xlsx");

    corpus();
}

/// The hardening corpus (`fixtures/corpus/`): one nasty trait per workbook.
/// `nasty_1904.xlsx` (1904 dates, cached formulas, error cells) is handcrafted by
/// `scripts/make_nasty_1904.py`; `not_a_workbook.xlsx` is plain text.
fn corpus() {
    let dir = "fixtures/corpus";

    // Merged region: only the top-left cell carries the value.
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Merged").unwrap();
    for (c, h) in ["a", "b", "c"].iter().enumerate() {
        ws.write_string(0, c as u16, *h).unwrap();
    }
    ws.write_number(1, 0, 1.0).unwrap();
    ws.merge_range(1, 1, 1, 2, "wide value", &Format::new())
        .unwrap();
    ws.write_number(2, 0, 2.0).unwrap();
    ws.write_string(2, 1, "x").unwrap();
    ws.write_string(2, 2, "y").unwrap();
    wb.save(format!("{dir}/merged.xlsx")).unwrap();

    // Display formats must not change raw values: currency + percent.
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Formats").unwrap();
    ws.write_string(0, 0, "price").unwrap();
    ws.write_string(0, 1, "growth").unwrap();
    let money = Format::new().set_num_format("$#,##0.00");
    let pct = Format::new().set_num_format("0.00%");
    ws.write_number_with_format(1, 0, 1234.5, &money).unwrap();
    ws.write_number_with_format(1, 1, 0.42, &pct).unwrap();
    wb.save(format!("{dir}/formats.xlsx")).unwrap();

    // Ragged rows: some rows shorter than the widest.
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Ragged").unwrap();
    for (c, h) in ["a", "b", "c"].iter().enumerate() {
        ws.write_string(0, c as u16, *h).unwrap();
    }
    ws.write_number(1, 0, 1.0).unwrap(); // row with only col a
    ws.write_number(2, 0, 2.0).unwrap();
    ws.write_number(2, 1, 20.0).unwrap();
    ws.write_number(2, 2, 200.0).unwrap(); // full row
    wb.save(format!("{dir}/ragged.xlsx")).unwrap();

    // Unicode / symbol-only sheet names.
    let mut wb = Workbook::new();
    wb.add_worksheet()
        .set_name("数据")
        .unwrap()
        .write_number(0, 0, 1.0)
        .unwrap();
    wb.add_worksheet()
        .set_name("Ümläute!")
        .unwrap()
        .write_number(0, 0, 2.0)
        .unwrap();
    wb.add_worksheet()
        .set_name("···")
        .unwrap()
        .write_number(0, 0, 3.0)
        .unwrap();
    wb.save(format!("{dir}/unicode.xlsx")).unwrap();

    // Format-only trailing cells widen the used range; ingestion must trim them.
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Padded").unwrap();
    ws.write_string(0, 0, "n").unwrap();
    ws.write_number(1, 0, 7.0).unwrap();
    ws.write_blank(9, 4, &Format::new().set_num_format("0.00"))
        .unwrap();
    wb.save(format!("{dir}/padded.xlsx")).unwrap();

    // Row-cap fixture: 20 data rows for --max-rows tests.
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Big").unwrap();
    ws.write_string(0, 0, "n").unwrap();
    for r in 1..=20u32 {
        ws.write_number(r, 0, r as f64).unwrap();
    }
    wb.save(format!("{dir}/rowcap.xlsx")).unwrap();

    eprintln!("wrote {dir}/{{merged,formats,ragged,unicode,padded,rowcap}}.xlsx");
}
