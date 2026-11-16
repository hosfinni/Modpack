#!/usr/bin/env python3
"""Move the whole pack to another Horizon commit.

    python3 tools/repin.py <40-hex Horizon commit> [--hooks-api N] [--mixins <40-hex commit>]

A native mod builds on one exact Horizon commit, and every mod of a build must name
the same one. This rewrites the pin everywhere it is written: `modpack.toml`, each
bundle's `horizon-mixin.toml` and each `api/Cargo.toml`. Then check out that commit in
your Horizon checkout and run `python3 tools/horizon.py check`: where Horizon renamed
or changed something a mod uses, the compiler says so.

`--hooks-api` is that commit's `NATIVE_HOOKS_API` (in
`crates/horizon-client/src/game/native_hooks.rs`); `--mixins` is the horizon-mixins
commit to build with. Both stay as they are when left out.
"""
import argparse
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
COMMIT = re.compile(r'[0-9a-f]{40}')


def replace(path, old, new):
    text = path.read_text()
    if old not in text:
        raise SystemExit(f'{path}: {old} is not written there')
    path.write_text(text.replace(old, new))
    print(f'{path.relative_to(ROOT)}: {old[:12]} -> {new[:12]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('revision')
    parser.add_argument('--hooks-api', type=int)
    parser.add_argument('--mixins')
    args = parser.parse_args()
    for name, value in (('the Horizon commit', args.revision), ('--mixins', args.mixins)):
        if value is not None and not COMMIT.fullmatch(value):
            raise SystemExit(f'{name} is 40 lowercase hex digits, not {value!r}')
    manifest = ROOT / 'modpack.toml'
    pack = tomllib.loads(manifest.read_text())['horizon']
    old = pack['revision']
    if old != args.revision:
        replace(manifest, old, args.revision)
        for bundle in sorted((ROOT / 'native').glob('*/horizon-mixin.toml')):
            replace(bundle, old, args.revision)
            replace(bundle.parent / 'api' / 'Cargo.toml', old, args.revision)
    if args.hooks_api is not None and args.hooks_api != pack['hooks_api']:
        replace(manifest, f'hooks_api = {pack["hooks_api"]}', f'hooks_api = {args.hooks_api}')
    if args.mixins is not None and args.mixins != pack['mixins_revision']:
        replace(manifest, pack['mixins_revision'], args.mixins)
    return 0


if __name__ == '__main__':
    sys.exit(main())
