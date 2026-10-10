"""Draw LeafPDF's project icon from geometric shapes, with no external assets."""
from pathlib import Path
from PIL import Image, ImageDraw

SIZE = 1024
image = Image.new("RGBA", (SIZE, SIZE))
draw = ImageDraw.Draw(image)
draw.rounded_rectangle((40, 40, 984, 984), radius=208, fill="#163c38")
draw.rounded_rectangle((236, 184, 748, 856), radius=52, fill="#eaf4ed")
draw.line((326, 374, 658, 374), fill="#b4d2c0", width=34)
draw.line((326, 470, 590, 470), fill="#b4d2c0", width=34)
draw.line((326, 566, 512, 566), fill="#b4d2c0", width=34)
draw.ellipse((442, 470, 842, 870), fill="#67ac80")
draw.polygon(((434, 884), (640, 624), (846, 446), (750, 730)), fill="#67ac80")
draw.line((466, 832, 780, 526), fill="#245d45", width=26)
draw.line((596, 706, 606, 590), fill="#245d45", width=20)
draw.line((660, 644, 768, 648), fill="#245d45", width=20)
target = Path(__file__).resolve().parents[1] / "assets/leafpdf.ico"
image.resize((256, 256), Image.Resampling.LANCZOS).save(
    target, format="ICO", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
)
print("Generated assets/leafpdf.ico")
