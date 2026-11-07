# SPDX-License-Identifier: AGPL-3.0-or-later
import struct
import io
import colorsys
from PIL import Image

def recolor_pixel_bgra(b, g, r, a):
    if a == 0:
        return (b, g, r, a)
    
    # RGB to HSV
    h, s, v = colorsys.rgb_to_hsv(r / 255.0, g / 255.0, b / 255.0)
    
    # 파란색/보라색 계열 (Hue 0.50 ~ 0.85) 및 채도가 있는 픽셀
    if s > 0.12 and 0.50 <= h <= 0.85:
        target_hue = 11.0 / 360.0 # Rust Red (~11°)
        new_s = min(1.0, s * 1.05)
        new_r, new_g, new_b = colorsys.hsv_to_rgb(target_hue, new_s, v)
        return (int(new_b * 255), int(new_g * 255), int(new_r * 255), a)
    else:
        return (b, g, r, a)

def recolor_png_bytes(png_bytes):
    img = Image.open(io.BytesIO(png_bytes)).convert("RGBA")
    data = img.getdata()
    new_data = []
    target_hue = 11.0 / 360.0
    for r, g, b, a in data:
        if a == 0:
            new_data.append((r, g, b, 0))
            continue
        h, s, v = colorsys.rgb_to_hsv(r / 255.0, g / 255.0, b / 255.0)
        if s > 0.12 and 0.50 <= h <= 0.85:
            new_s = min(1.0, s * 1.05)
            nr, ng, nb = colorsys.hsv_to_rgb(target_hue, new_s, v)
            new_data.append((int(nr * 255), int(ng * 255), int(nb * 255), a))
        else:
            new_data.append((r, g, b, a))
    
    out_img = Image.new("RGBA", img.size)
    out_img.putdata(new_data)
    buf = io.BytesIO()
    out_img.save(buf, format="PNG", optimize=True)
    return buf.getvalue()

def process_ico(src_ico_path, dst_ico_path):
    with open(src_ico_path, "rb") as f:
        data = bytearray(f.read())
    
    res, ico_type, count = struct.unpack("<HHH", data[:6])
    print(f"Total icon images in original ICO: {count}")

    # 아이콘 디렉토리 엔트리들을 수정할 수 있도록 파싱
    entries = []
    for i in range(count):
        entry_offset = 6 + i * 16
        w, h, c, r, planes, bpp, size, offset = struct.unpack("<BBBBHHII", data[entry_offset:entry_offset+16])
        entries.append({
            "index": i,
            "entry_offset": entry_offset,
            "w": w if w != 0 else 256,
            "h": h if h != 0 else 256,
            "planes": planes,
            "bpp": bpp,
            "size": size,
            "offset": offset,
            "is_png": data[offset:offset+4] == b"\x89PNG"
        })

    # DIB 아이콘들(Icon 0 ~ 5)은 제자리에서 BGRA 픽셀만 Rust Red로 변환
    # DIB의 헤더: BITMAPINFOHEADER (40 바이트)
    for entry in entries:
        offset = entry["offset"]
        if not entry["is_png"]:
            header_size = struct.unpack("<I", data[offset:offset+4])[0]
            w = entry["w"]
            h = entry["h"]
            pixel_bytes_start = offset + header_size
            pixel_count = w * h
            
            # BGRA 픽셀 변환
            for p in range(pixel_count):
                idx = pixel_bytes_start + p * 4
                b = data[idx]
                g = data[idx+1]
                r = data[idx+2]
                a = data[idx+3]
                nb, ng, nr, na = recolor_pixel_bgra(b, g, r, a)
                data[idx] = nb
                data[idx+1] = ng
                data[idx+2] = nr
                data[idx+3] = na
            print(f"Recolored DIB Icon {entry['index']} ({w}x{h}) preserving native 1-bit AND transparency mask.")
        else:
            # PNG 아이콘 (Icon 6, 256x256)
            png_data = bytes(data[offset:offset+entry["size"]])
            new_png = recolor_png_bytes(png_data)
            # 새 PNG 크기가 다를 수 있으므로 ICO 재구성
            entry["new_png"] = new_png

    # PNG 데이터가 있는 경우 최종 바이너리 빌드
    new_ico = bytearray()
    new_ico.extend(data[:6]) # 헤더
    
    current_data_offset = 6 + count * 16
    for entry in entries:
        if "new_png" in entry:
            size = len(entry["new_png"])
        else:
            size = entry["size"]
        
        # 디렉토리 엔트리 작성
        w_byte = entry["w"] if entry["w"] < 256 else 0
        h_byte = entry["h"] if entry["h"] < 256 else 0
        new_ico.extend(struct.pack("<BBBBHHII", w_byte, h_byte, 0, 0, entry["planes"], entry["bpp"], size, current_data_offset))
        current_data_offset += size

    # 이미지 데이터 작성
    for entry in entries:
        if "new_png" in entry:
            new_ico.extend(entry["new_png"])
        else:
            new_ico.extend(data[entry["offset"]:entry["offset"]+entry["size"]])

    with open(dst_ico_path, "wb") as f:
        f.write(new_ico)
    print(f"Successfully generated native Windows ICO with transparent Task Manager support: {dst_ico_path} ({len(new_ico)} bytes)")

def process_tray_png(src_png_path, dst_png_path):
    with open(src_png_path, "rb") as f:
        raw = f.read()
    recolored = recolor_png_bytes(raw)
    with open(dst_png_path, "wb") as f:
        f.write(recolored)
    print(f"Successfully generated 32x32 Rust Red Tray Icon: {dst_png_path} ({len(recolored)} bytes)")

if __name__ == "__main__":
    src_ico = "fluxer_desktop/build_resources/icons-stable/icon.ico"
    dst_ico = "fluxer_desktop_rs/resources/icon.ico"
    src_tray = "fluxer_desktop/build_resources/icons-stable/32x32.png"
    dst_tray = "fluxer_desktop_rs/resources/tray_32x32.png"
    
    process_ico(src_ico, dst_ico)
    process_tray_png(src_tray, dst_tray)
