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
}
