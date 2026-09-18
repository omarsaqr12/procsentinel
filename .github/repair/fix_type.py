from pathlib import Path
p = Path('src/criu_manager.rs')
s = p.read_text()
old = '                let mut checkpoint_id = None;\n'
new = '                let mut checkpoint_id: Option<String> = None;\n'
if s.count(old) != 1:
    raise SystemExit(f'expected one checkpoint_id declaration, found {s.count(old)}')
p.write_text(s.replace(old, new, 1))
