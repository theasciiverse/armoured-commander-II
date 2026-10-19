"""Writes data/engine_defs.json: the constants at the top of armcom2.py
(lines from "Constants" to "Classes": colours, hex geometry, the engine's
tables), run against a stub libtcod. Rerun it rather than editing the JSON.

    python3 tools/defs2json.py "../../references/Armoured Commander II/armcom2.py"
"""
import json, sys, types

src = open(sys.argv[1], encoding='utf8').read().split('\n')
start = next(i for i, l in enumerate(src) if l.startswith('NAME = '))
end = next(i for i, l in enumerate(src) if l.startswith('class Campaign'))
code = '\n'.join(src[start:end])

class Color(tuple):
    def __new__(cls, r, g, b):
        return tuple.__new__(cls, (r, g, b))

# libtcod's named colours, as the shipped libtcodpy_local defines them
import re, os
lib = types.SimpleNamespace(Color=Color)
tcod_init = os.path.join(os.path.dirname(sys.argv[1]), 'libtcodpy_local', '__init__.py')
for name, r, g, b in re.findall(r'^(\w+)=Color\((\d+),(\d+),(\d+)\)', open(tcod_init).read(), re.M):
    setattr(lib, name, Color(int(r), int(g), int(b)))
for attr in ['RENDERER_SDL2', 'RENDERER_SDL', 'RENDERER_OPENGL2']:
    setattr(lib, attr, 0)
env = {'libtcod': lib, 'os': __import__('os'), 'sys': sys, 'DEBUG': False}
exec(code, env)

out = {}
for k, v in env.items():
    if not k.isupper() or k.startswith('_'):
        continue
    try:
        json.dumps(v)
    except TypeError:
        continue
    out[k] = v

def keys_to_str(o):
    if isinstance(o, dict):
        return {str(k): keys_to_str(v) for k, v in o.items()}
    if isinstance(o, (list, tuple)):
        return [keys_to_str(x) for x in o]
    return o

json.dump(keys_to_str(out), open('data/engine_defs.json', 'w'), indent=1, ensure_ascii=False)
print(len(out), 'constants:', ', '.join(sorted(out)))
