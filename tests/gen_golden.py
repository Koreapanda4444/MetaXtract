import os
import subprocess

FIXTURES_DIR = os.path.join(os.path.dirname(__file__), 'fixtures')
GOLDEN_DIR = os.path.join(os.path.dirname(__file__), 'golden')

FIXTURE_FILES = [
    'image_gps.jpg',
    'image_noexif.jpg',
    'sample.pdf',
    'sample.docx',
    'sample.mp4',
]

os.makedirs(GOLDEN_DIR, exist_ok=True)

for fixture_file in FIXTURE_FILES:
    fixture_path = os.path.join(FIXTURES_DIR, fixture_file)
    golden_path = os.path.join(GOLDEN_DIR, f'scan_{fixture_file}.jsonl')
    print(f'Generating {golden_path} ...')
    subprocess.run([
        'python', '-m', 'metaxtract', 'scan', fixture_path, '--out', golden_path
    ], check=True)
