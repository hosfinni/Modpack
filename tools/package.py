#!/usr/bin/env python3
"""Pack every part of the modpack into the files Horizon's launcher and registry take.

    python3 tools/package.py            # writes dist/
    python3 tools/package.py --check    # and runs Horizon's own checks on each file

It writes, into `dist/`:

- `hz_content-<version>.hmod`: the content pack (its manifest, `assets/` and `data/`);
- `<id>-<version>.hnmod` for each native mod: `horizon-native.json` and the bundle's
  sources under `mixins/<id>/` (the launcher compiles them into the game);
- `hz_softlight-<version>.hshader`: the shader pack.

The archives have fixed dates and modes and sorted entries, so the same sources always
give the same bytes and the same SHA-256.

`--check` needs Horizon's `cargo-horizon-mod` (`cargo install --git
https://github.com/horizon-rust/horizon cargo-horizon-mod`, or `--checker PATH` to a
built binary): the same checks the registry runs on a submission.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / 'dist'
# What a native bundle may hold besides `.rs` and `.toml` (Horizon docs/api/mod-files.md).
NOTICES = ('README', 'LICENSE', 'LICENCE', 'COPYING', 'NOTICE')
# Native mods that name the hook API they were made on instead of pinning a commit.
# None: the launcher then builds on the newest Nightly or release installed, and
# refuses one older than the hooks need. Pinned, every mod builds on the one commit
# in modpack.toml, whatever Horizon version is installed.
HOOKS_ONLY = ()


def write_zip(path, files):
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name, content in sorted(files.items()):
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, content)
    return path


def tree(folder, keep):
    files = {}
    for path in sorted(folder.rglob('*')):
        relative = path.relative_to(folder)
        if path.is_file() and 'target' not in relative.parts and keep(relative):
            files[relative.as_posix()] = path.read_bytes()
    return files


def content_pack():
    folder = ROOT / 'content' / 'hz_content'
    manifest = tomllib.loads((folder / 'horizon-mod.toml').read_text())
    files = tree(folder, lambda p: p.parts[0] in ('assets', 'data') or p.as_posix() == 'horizon-mod.toml')
    return write_zip(DIST / f"{manifest['id']}-{manifest['version']}.hmod", files)


def native_mods(pack):
    out = []
    for manifest_path in sorted((ROOT / 'native').glob('*/horizon-mixin.toml')):
        bundle = manifest_path.parent
        mixin = tomllib.loads(manifest_path.read_text())
        mod = mixin['mod']
        if bundle.name != mod['id']:
            raise SystemExit(f'{bundle}: the folder must be named after [mod] id ({mod["id"]})')
        pinned = mixin.get('compatibility', {}).get('horizon_revision')
        if pinned != pack['horizon']['revision']:
            raise SystemExit(f'{manifest_path} pins {pinned}, modpack.toml pins {pack["horizon"]["revision"]}')
        if pack['native'].get(mod['id']) != mod['version']:
            raise SystemExit(f'modpack.toml lists {mod["id"]} {pack["native"].get(mod["id"])}, '
                             f'its bundle is {mod["version"]}')
        native = dict(schema=1, id=mod['id'], version=mod['version'], game=pack['modpack']['game'],
                      horizon=pinned, mixins=pack['horizon']['mixins_revision'])
        if mod['id'] in HOOKS_ONLY:
            native['hooks'] = pack['horizon']['hooks_api']

        def allowed(path):
            return (path.suffix in ('.rs', '.toml') or 'data' in path.parts[:2]
                    or path.name.startswith(NOTICES) or path.name == '.gitignore')

        files = {f'mixins/{mod["id"]}/{name}': content for name, content in tree(bundle, allowed).items()}
        files['horizon-native.json'] = (json.dumps(native, indent=2, sort_keys=True) + '\n').encode()
        out.append(write_zip(DIST / f"{mod['id']}-{mod['version']}.hnmod", files))
    return out


def shader_pack():
    folder = ROOT / 'shaders' / 'hz_softlight'
    manifest = tomllib.loads((folder / 'horizon-shaders.toml').read_text())
    files = tree(folder, lambda p: p.suffix in ('.wgsl', '.toml', '.png'))
    return write_zip(DIST / f"{manifest['id']}-{manifest['version']}.hshader", files)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--check', action='store_true', help="run Horizon's checks on each file")
    parser.add_argument('--checker', help='a cargo-horizon-mod binary (default: the one on PATH)')
    args = parser.parse_args()
    pack = tomllib.loads((ROOT / 'modpack.toml').read_text())
    shutil.rmtree(DIST, ignore_errors=True)
    written = [content_pack(), *native_mods(pack), shader_pack()]
    for path in written:
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f'{path.relative_to(ROOT)}  {path.stat().st_size} bytes  sha256 {digest}')
    if not args.check:
        return 0
    checker = args.checker or shutil.which('cargo-horizon-mod')
    if not checker:
        raise SystemExit('--check needs cargo-horizon-mod on PATH, or --checker PATH')
    failed = 0
    for path in written:
        result = subprocess.run([checker, 'horizon-mod', 'check', str(path)], capture_output=True, text=True)
        print(f'\n$ cargo horizon-mod check {path.name}\n{(result.stdout + result.stderr).rstrip()}')
        failed += result.returncode != 0
    if failed:
        print(f'\n{failed} of {len(written)} files failed the check')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
