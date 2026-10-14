"""Generate the local-only MagicRenamer fixture corpus using Python's stdlib."""
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile
from xml.sax.saxutils import escape
import json

ROOT = Path(__file__).parent


def write(relative_path: str, content: str) -> None:
    path = ROOT / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content.rstrip() + "\n", encoding="utf-8")


# These representative files are referenced by the Rust fixture tests.
write("download_1.txt", """Invoice Number: INV-2026-0812
Date: August 12, 2026
Vendor: CloudScale Infrastructure Inc.
Customer: Example Development Lab
Description: Local GPU compute usage
Total Due: EUR 450.00
Payment Status: Paid""")
write("untitled_notes.md", """# Product Planning Notes
Meeting Date: 2026-09-03
Decision: keep document processing local.
Action: prepare an offline archive workflow.""")
write("finance/invoices/2026-08-01_northstar.txt", """Invoice INV-8041
Date: 2026-08-01
Vendor: Northstar Office Supply
Total: EUR 418.20
Status: Paid""")
write("finance/invoices/2026-08-14_CedarCloud_INV-1092.md", """# Cloud Hosting Invoice
Date: 2026-08-14
Vendor: Cedar Cloud
Invoice: INV-1092
Total: EUR 1,240.00""")
write("finance/invoices/2026-09-02_Harbor_Invoice_772.csv", """invoice,date,vendor,total,currency
772,2026-09-02,Harbor Analytics,825.00,EUR
773,2026-09-03,Harbor Analytics,190.00,EUR""")
write("finance/invoices/invoice_003.pdf.txt", """Invoice number: 003
Date: 2026-09-18
Vendor: Alpine Freight
Amount due: EUR 96.40""")
write("finance/invoices/2026-10-05_Redwood_Invoice.json", json.dumps({
    "invoice_number": "RW-221", "date": "2026-10-05", "vendor": "Redwood Design",
    "currency": "EUR", "total": 3180.0,
    "items": [{"description": "Brand workshop"}, {"description": "Visual identity"}],
}, indent=2))
write("finance/invoices/invoice_acme_2026-10-21.md", """# Acme Parts Invoice
Date: 2026-10-21
Invoice No: AC-5107
Total: EUR 967.20""")
write("engineering/meeting-notes/2026-08-05_platform_sync.md", """# Platform Sync
Date: 2026-08-05
Attendees: Platform Team
Decision: move the cache service to a local-first design.
Action: draft migration steps.""")

# Add 42 more small, varied files across sibling folders and formats.
collections = {
    "finance/receipts": ("receipt", "Merchant receipt", "paid"),
    "engineering/release-records": ("release", "Release record", "stable"),
    "legal/contracts": ("agreement", "Contract summary", "review"),
    "research/field-studies": ("field-note", "Field observation", "sampled"),
    "operations/maintenance": ("work-order", "Maintenance record", "complete"),
    "data-exports": ("report", "Data export summary", "generated"),
    "support/tickets": ("ticket", "Support case summary", "resolved"),
}
extensions = ("txt", "md", "csv", "json", "txt", "md")
for folder, (kind, title, status) in collections.items():
    for index, extension in enumerate(extensions, 1):
        date = f"2026-{8 + (index - 1) // 3:02d}-{index * 3:02d}"
        stem = f"{date}_{kind}_{index:03d}"
        if extension == "json":
            content = json.dumps({"id": f"{kind.upper()}-{index:03d}", "date": date,
                                  "title": title, "status": status,
                                  "records": [{"sequence": index, "value": "sample"}]}, indent=2)
        elif extension == "csv":
            content = f"date,reference,title,status\n{date},{kind.upper()}-{index:03d},{title},{status}"
        elif extension == "md":
            content = f"# {title} {index:03d}\nDate: {date}\nStatus: {status}\nSummary: Small local fixture for naming review."
        else:
            content = f"{title}\nDate: {date}\nReference: {kind.upper()}-{index:03d}\nStatus: {status}\nSummary: Small local fixture for naming review."
        write(f"{folder}/{stem}.{extension}", content)


def make_docx(relative_path: str, title: str, lines: list[str]) -> None:
    path = ROOT / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    paragraphs = "".join(
        f'<w:p><w:r><w:t xml:space="preserve">{escape(line)}</w:t></w:r></w:p>'
        for line in [title, *lines]
    )
    document = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
        f"<w:body>{paragraphs}<w:sectPr/></w:body></w:document>"
    )
    with ZipFile(path, "w", ZIP_DEFLATED) as archive:
        archive.writestr("[Content_Types].xml", '''<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>''')
        archive.writestr("_rels/.rels", '''<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>''')
        archive.writestr("word/document.xml", document)


def make_pdf(relative_path: str, title: str, lines: list[str]) -> None:
    path = ROOT / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)

    def pdf_text(value: str) -> str:
        return (value.encode("ascii", "replace").decode("ascii")
                .replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)"))

    commands = ["BT", "/F1 12 Tf", "72 740 Td"]
    for index, line in enumerate([title, *lines]):
        if index:
            commands.append("0 -18 Td")
        commands.append(f"({pdf_text(line)}) Tj")
    stream = "\n".join([*commands, "ET"]).encode("ascii")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
    ]
    output = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f"{index} 0 obj\n".encode())
        output.extend(obj + b"\nendobj\n")
    xref_offset = len(output)
    output.extend(f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010d} 00000 n \n".encode())
    output.extend(f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n".encode())
    path.write_bytes(output)


make_docx("document-formats/2026-09-06_Project_Brief.docx", "Project Brief: Offline Archive", [
    "Date: 2026-09-06", "Goal: index a local archive without uploading document contents.",
    "Owner: Product Engineering",
])
make_docx("document-formats/meeting_minutes_2026-10-11.docx", "Meeting Minutes", [
    "Date: 2026-10-11", "Decision: retain timestamped rename manifests.",
])
make_docx("document-formats/vendor_contract_summary.docx", "Vendor Contract Summary", [
    "Vendor: Harbor Analytics", "Effective date: 2026-10-18",
])
make_pdf("document-formats/2026-08-16_Lab_Results.pdf", "Laboratory Results", [
    "Sample date: 2026-08-16", "Study: North Harbor water quality", "Result: within expected range",
])
make_pdf("document-formats/receipt_2026-09-28.pdf", "Purchase Receipt", [
    "Date: 2026-09-28", "Merchant: Cedar Stationery", "Total: EUR 24.60",
])

# 504 top-level files plus 18 files in six child folders = 522 discovered files.
large = ROOT / "large-batch"
large.mkdir(parents=True, exist_ok=True)
vendors = ("Northstar", "CedarCloud", "Harbor", "Redwood", "Alpine", "Westfield")
topics = ("invoice", "service-report", "meeting-note", "receipt", "field-observation", "work-order")
for index in range(1, 505):
    date = f"2026-{((index - 1) % 12) + 1:02d}-{((index - 1) % 27) + 1:02d}"
    vendor = vendors[(index - 1) % len(vendors)]
    topic = topics[(index - 1) % len(topics)]
    stem = f"{date}_{vendor}_{topic}_{index:04d}"
    if index % 4 == 0:
        extension = "csv"
        content = f"date,reference,category,amount\n{date},REF-{index:04d},{topic},{(index * 7) % 950 + 25}.00"
    elif index % 4 == 1:
        extension = "md"
        content = f"# {topic.replace('-', ' ').title()} {index:04d}\nDate: {date}\nOrganization: {vendor}\nSummary: Small fixture for queue and context review."
    elif index % 4 == 2:
        extension = "json"
        content = json.dumps({"id": f"DOC-{index:04d}", "date": date, "organization": vendor,
                              "category": topic, "tags": ["fixture", "large-batch"],
                              "records": [{"sequence": index, "status": "sample"}]}, indent=2)
    else:
        extension = "txt"
        content = f"{topic.replace('-', ' ').title()}\nDate: {date}\nOrganization: {vendor}\nReference: DOC-{index:04d}\nPurpose: exercise recursive scanning and compact excerpts."
    write(f"large-batch/{stem}.{extension}", content)

for folder in ("archive", "incoming", "reports", "receipts", "notes", "research"):
    for index in range(1, 4):
        number = len(folder) * 100 + index
        write(f"large-batch/{folder}/{folder}_{number:04d}.txt",
              f"{folder.title()} Sample\nDate: 2026-10-{index + 20:02d}\nReference: {folder[:3].upper()}-{number:04d}\nNested recursive-scan fixture.")

print(f"Generated fixture corpus under {ROOT}")
print("Large-batch files: 522")
