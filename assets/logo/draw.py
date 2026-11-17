from PIL import Image, ImageDraw
import math
S = 2048  # supersampled canvas; downscaled at the end
BG = (17, 18, 20); GOLD = (242, 178, 51); GOLD_D = (196, 136, 26); INK = (22, 22, 22); WHITE = (250, 248, 242)

def logo(size, simple=False):
    im = Image.new("RGBA", (S, S), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
    pad = S * 0.04
    d.rounded_rectangle((pad, pad, S - pad, S - pad), radius=S * 0.23, fill=BG)
    # soft gold glow behind the head
    glow = Image.new("RGBA", (S, S), (0, 0, 0, 0)); g = ImageDraw.Draw(glow)
    for i in range(40, 0, -1):
        r = S * 0.30 * i / 40
        g.ellipse((S*0.47 - r, S*0.53 - r, S*0.47 + r, S*0.53 + r), fill=(242, 178, 51, int(5 * (1 - i/40) * 6)))
    im = Image.alpha_composite(im, glow); d = ImageDraw.Draw(im)
    cx, cy, r = S * 0.46, S * 0.56, S * 0.25
    o = S * 0.028  # outline width
    # head
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=WHITE, outline=INK, width=int(o))
    # eyes (big ovals like the cast) + shine
    for ex in (cx - r * 0.18, cx + r * 0.36):
        ew, eh = r * 0.13, r * 0.24
        d.ellipse((ex - ew, cy - r*0.12 - eh, ex + ew, cy - r*0.12 + eh), fill=INK)
        d.ellipse((ex - ew*0.15, cy - r*0.12 - eh*0.75, ex + ew*0.55, cy - r*0.12 - eh*0.25), fill=WHITE)
    # smile
    d.arc((cx - r*0.05, cy + r*0.05, cx + r*0.6, cy + r*0.55), start=20, end=160, fill=INK, width=int(o*0.9))
    # graduation cap: board (diamond) + skull cap + tassel
    top = cy - r * 0.88
    cap = [(cx - r*0.05, top - r*0.42), (cx + r*1.12, top - r*0.08), (cx + r*0.05, top + r*0.30), (cx - r*1.12, top - r*0.04)]
    d.polygon([(cx - r*0.62, top + r*0.05), (cx + r*0.62, top + r*0.12), (cx + r*0.55, top + r*0.42), (cx - r*0.58, top + r*0.36)], fill=GOLD_D, outline=INK, width=int(o*0.8))
    d.polygon(cap, fill=GOLD, outline=INK, width=int(o*0.8))
    if not simple:
        tx, ty = cx + r*1.0, top - r*0.08
        d.line((cx - r*0.02, top - r*0.05, tx, ty), fill=INK, width=int(o*0.6))
        d.line((tx, ty, tx + r*0.02, ty + r*0.55), fill=INK, width=int(o*0.6))
        d.ellipse((tx - r*0.09, ty + r*0.5, tx + r*0.13, ty + r*0.72), fill=GOLD, outline=INK, width=int(o*0.5))
    # play badge, bottom right
    bx, by, br = S * 0.735, S * 0.745, S * 0.155
    d.ellipse((bx - br, by - br, bx + br, by + br), fill=GOLD, outline=BG, width=int(S*0.025))
    t = br * 0.48
    d.polygon([(bx - t*0.7, by - t), (bx - t*0.7, by + t), (bx + t*1.05, by)], fill=BG)
    return im.resize((size, size), Image.LANCZOS)

out = "F:/Tools/lazylearn-logo/"
big = logo(1024); big.save(out + "lazylearn-1024.png")
for s in (512, 256, 128, 64, 48, 32, 16):
    logo(s, simple=s <= 48).save(out + f"lazylearn-{s}.png")
ico_imgs = [logo(s, simple=s <= 48) for s in (256, 128, 64, 48, 32, 24, 16)]
ico_imgs[0].save(out + "lazylearn.ico", sizes=[(s, s) for s in (256, 128, 64, 48, 32, 24, 16)], append_images=ico_imgs[1:])
# preview sheet on light and dark backgrounds
sheet = Image.new("RGB", (1400, 560), (235, 235, 235))
sheet.paste(big.resize((512, 512), Image.LANCZOS), (20, 24), big.resize((512, 512), Image.LANCZOS))
x = 560
for s in (256, 128, 64, 48, 32, 16):
    im = Image.open(out + f"lazylearn-{s}.png"); sheet.paste(im, (x, 200 - s // 2 + 60), im); x += s + 24
dark = Image.new("RGB", (1400, 140), (30, 30, 34)); x = 560
for s in (64, 48, 32, 16):
    im = Image.open(out + f"lazylearn-{s}.png"); dark.paste(im, (x, 70 - s // 2), im); x += s + 30
sheet.paste(dark.crop((540, 0, 1400, 140)), (540, 410))
sheet.save(out + "preview.png"); print("ok")
