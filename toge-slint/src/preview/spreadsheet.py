import sys, zipfile, xml.etree.ElementTree as ET, posixpath

LIMIT = 64 * 1024
output = bytearray()
truncated = False

def add(value):
    global truncated
    encoded = value.encode('utf-8')
    room = LIMIT - len(output)
    if len(encoded) > room:
        output.extend(encoded[:room].decode('utf-8', 'ignore').encode('utf-8'))
        truncated = True
    else:
        output.extend(encoded)

def clean(value):
    return value.replace('\n', ' ').replace('\r', ' ').replace('\t', ' ')[:512]

with zipfile.ZipFile(sys.argv[1]) as archive:
    budget = 32 * 1024 * 1024
    def xml(member):
        global budget
        info = archive.getinfo(member)
        if info.file_size > min(budget, 8 * 1024 * 1024):
            raise ValueError('spreadsheet XML too large')
        budget -= info.file_size
        return ET.fromstring(archive.read(member))
    if sys.argv[1].lower().endswith('.xlsx'):
        ns = {'s': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
        rels = {node.get('Id'): node.get('Target') for node in xml('xl/_rels/workbook.xml.rels')}
        strings = []
        if 'xl/sharedStrings.xml' in archive.namelist():
            strings = [''.join(node.itertext()) for node in xml('xl/sharedStrings.xml').findall('s:si', ns)]
        for sheet in xml('xl/workbook.xml').findall('s:sheets/s:sheet', ns)[:8]:
            add('\nSheet: ' + sheet.get('name', 'Unnamed') + '\n')
            key = sheet.get('{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id')
            target = rels.get(key, '')
            member = target.lstrip('/') if target.startswith('/') else posixpath.normpath('xl/' + target)
            if not member.startswith('xl/'):
                raise ValueError('invalid sheet relationship')
            for row in xml(member).findall('s:sheetData/s:row', ns)[:200]:
                values = []
                for cell in row.findall('s:c', ns)[:30]:
                    value = cell.findtext('s:v', default='', namespaces=ns)
                    kind = cell.get('t')
                    if kind == 's': value = strings[int(value)]
                    elif kind == 'inlineStr': value = ''.join(cell.find('s:is', ns).itertext())
                    elif kind == 'b': value = 'TRUE' if value == '1' else 'FALSE'
                    if not value and cell.find('s:f', ns) is not None: value = '=' + cell.findtext('s:f', namespaces=ns)
                    values.append(cell.get('r', '?') + ': ' + clean(value))
                add(' | '.join(values) + '\n')
                if truncated: break
            if truncated: break
    else:
        table = '{urn:oasis:names:tc:opendocument:xmlns:table:1.0}'
        office = '{urn:oasis:names:tc:opendocument:xmlns:office:1.0}'
        for sheet_index, sheet in enumerate(xml('content.xml').iter(table + 'table')):
            if sheet_index >= 8: break
            add('\nSheet: ' + sheet.get(table + 'name', 'Unnamed') + '\n')
            row_number = 0
            for row in sheet.iter(table + 'table-row'):
                repeat = min(int(row.get(table + 'number-rows-repeated', '1')), 200 - row_number)
                values = []
                for cell in row:
                    if cell.tag not in (table + 'table-cell', table + 'covered-table-cell'): continue
                    value = ''.join(cell.itertext()).strip() or cell.get(office + 'value', '') or cell.get(office + 'boolean-value', '')
                    count = min(int(cell.get(table + 'number-columns-repeated', '1')), 30 - len(values))
                    values.extend([clean(value)] * count)
                    if len(values) >= 30: break
                if any(values):
                    for _ in range(repeat):
                        row_number += 1
                        add(str(row_number) + ': ' + ' | '.join(values) + '\n')
                        if truncated: break
                else:
                    row_number += repeat
                if row_number >= 200 or truncated: break
            if truncated: break
add('\nData preview: up to 8 sheets, 200 rows and 30 columns per sheet.\nStored values; formatting and formula recalculation are not included.')
if truncated: output.extend(b'\n... Preview limited to the first 64 KiB')
sys.stdout.buffer.write(output)
