
from pathlib import Path


def ensure_dir(p: Path) -> None:
    p.mkdir(parents=True, exist_ok=True)


def make_jpeg_noexif(path: Path) -> None:
    from PIL import Image
    img = Image.new("RGB", (64, 64), (120, 180, 240))
    img.save(path, "JPEG", quality=85)


def make_jpeg_with_fake_gps(path: Path) -> None:
    from PIL import Image
    import piexif
    img = Image.new("RGB", (64, 64), (200, 120, 120))
    zeroth_ifd = {}
    exif_ifd = {}
    gps_ifd = {
        piexif.GPSIFD.GPSLatitudeRef: "N",
        piexif.GPSIFD.GPSLatitude: ((0, 1), (0, 1), (0, 1)),
        piexif.GPSIFD.GPSLongitudeRef: "E",
        piexif.GPSIFD.GPSLongitude: ((0, 1), (0, 1), (0, 1)),
    }
    exif_dict = {"0th": zeroth_ifd, "Exif": exif_ifd, "GPS": gps_ifd, "1st": {}, "thumbnail": None}
    exif_bytes = piexif.dump(exif_dict)
    img.save(path, "JPEG", quality=85, exif=exif_bytes)


def make_pdf(path: Path) -> None:
    from reportlab.pdfgen import canvas
    c = canvas.Canvas(str(path))
    c.drawString(72, 720, "MetaXtract test PDF")
    c.save()


def make_docx(path: Path) -> None:
    from docx import Document
    doc = Document()
    doc.add_paragraph("MetaXtract test DOCX")
    doc.core_properties.author = "MetaXtract"
    doc.save(str(path))

def make_mp4_with_ffmpeg(path: Path) -> bool:
    import shutil
    import subprocess
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        return False
    cmd = [
        ffmpeg, "-y",
        "-f", "lavfi", "-i", "color=c=black:s=320x240:d=1",
        "-pix_fmt", "yuv420p",
        str(path)
    ]
    subprocess.run(cmd, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return path.exists() and path.stat().st_size > 0

def generate(fixtures_dir: str) -> dict:
    d = Path(fixtures_dir)
    ensure_dir(d)
    out = {
        "image_gps.jpg": False,
        "image_noexif.jpg": False,
        "sample.pdf": False,
        "sample.docx": False,
        "sample.mp4": False,
    }
    p = d / "image_noexif.jpg"
    if not p.exists():
        make_jpeg_noexif(p)
    out["image_noexif.jpg"] = p.exists()
    p = d / "image_gps.jpg"
    if not p.exists():
        make_jpeg_with_fake_gps(p)
    out["image_gps.jpg"] = p.exists()
    p = d / "sample.pdf"
    if not p.exists():
        make_pdf(p)
    out["sample.pdf"] = p.exists()
    p = d / "sample.docx"
    if not p.exists():
        make_docx(p)
    out["sample.docx"] = p.exists()
    p = d / "sample.mp4"
    if not p.exists():
        out["sample.mp4"] = make_mp4_with_ffmpeg(p)
    else:
        out["sample.mp4"] = True
    return out
