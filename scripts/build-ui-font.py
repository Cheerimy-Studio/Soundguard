"""Rebuild the bundled UI fallback font: pip install 'fonttools[woff]'"""
from pathlib import Path
from fontTools import subset
import shutil
root=Path(__file__).resolve().parents[1]
chars=set(chr(i) for i in range(32,256))
for code in range(0x4e00,0xa000):
    try: chr(code).encode('gb2312');chars.add(chr(code))
    except UnicodeEncodeError: pass
for part in ['web/src','server/src','mobile/android/app/src/main']:
    for p in (root/part).rglob('*'):
        if p.suffix in {'.ts','.tsx','.js','.html','.rs','.java','.css'}:
            chars.update(p.read_text(encoding='utf-8'))
options=subset.Options();options.flavor='woff2';options.layout_features=['*']
font=subset.load_font(str(root/'client-native/assets/NotoSansCJKsc-Regular.otf'),options)
sub=subset.Subsetter(options=options);sub.populate(text=''.join(sorted(chars)));sub.subset(font)
canonical=root/'web/public/fonts/SoundGuardCJK.woff2';canonical.parent.mkdir(parents=True,exist_ok=True)
subset.save_font(font,str(canonical),options)
for folder in ['web/public/fonts','mobile/android/app/src/main/assets/fonts']:
    target=root/folder;target.mkdir(parents=True,exist_ok=True)
    if target/'SoundGuardCJK.woff2'!=canonical: shutil.copyfile(canonical,target/'SoundGuardCJK.woff2')
    shutil.copyfile(root/'client-native/assets/OFL.txt',target/'OFL.txt')
print(f'UI font: {canonical.stat().st_size} bytes; {len(chars)} characters')
