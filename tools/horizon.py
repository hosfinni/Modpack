#!/usr/bin/env python3
"""Stage the native mods beside a Horizon checkout and type-check them against the game.

The Horizon launcher builds a trait mod by copying its `[api]` package into the game's
workspace as `native-api/<id>`, its `[[crate]]`s as `native-crates/<id>/<crate>`, and
pointing the package's `horizon-client` dependency at the game's own crate
(launcher/src/native_mods/api.rs). This script lays out the same thing in
`target/stage/` and runs Cargo there, so `check` answers "does every mod compile against
the pinned Horizon?" without the launcher, a game install or Minecraft's assets.

    python3 tools/horizon.py check                 # every mod
    python3 tools/horizon.py check hz_travel       # one mod
    python3 tools/horizon.py stage                 # only write target/stage/

The Horizon checkout is `--horizon PATH`, else `$HORIZON_SRC`, else `../horizon` or
`../upstream/horizon`. It must be at the commit `modpack.toml` pins; the script refuses
another one unless `--allow-revision-mismatch` is given. Nothing in the checkout is
changed.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
STAGE = ROOT / 'target' / 'stage'
# Horizon's own dev profile (its Cargo.toml): the build script that generates the mod
# API parses the whole game with syn, which is slow unoptimized.
PROFILE = '''
[profile.dev]
debug = "line-tables-only"
[profile.dev.package.horizon-mod-api-gen]
opt-level = 3
[profile.dev.package.syn]
opt-level = 3
[profile.dev.package.proc-macro2]
opt-level = 3
'''


def bundles(only):
    found = []
    for manifest in sorted((ROOT / 'native').glob('*/horizon-mixin.toml')):
        mixin = tomllib.loads(manifest.read_text())
        mod_id = mixin['mod']['id']
        if manifest.parent.name != mod_id:
            raise SystemExit(f'{manifest}: the folder must be named after [mod] id ({mod_id})')
        if only and mod_id not in only:
            continue
        found.append((mod_id, manifest.parent, mixin))
    missing = set(only) - {mod_id for mod_id, _, _ in found}
    if missing:
        raise SystemExit(f'no such mod: {", ".join(sorted(missing))}')
    return found


def horizon_checkout(args):
    candidates = [args.horizon, os.environ.get('HORIZON_SRC'), ROOT.parent / 'horizon',
                  ROOT.parent / 'upstream' / 'horizon']
    for candidate in candidates:
        if candidate and (Path(candidate) / 'crates' / 'horizon-client' / 'Cargo.toml').is_file():
            return Path(candidate).resolve()
    raise SystemExit('no Horizon checkout: pass --horizon PATH or set HORIZON_SRC '
                     '(git clone https://github.com/horizon-rust/horizon ../horizon)')


def pinned_revision():
    return tomllib.loads((ROOT / 'modpack.toml').read_text())['horizon']['revision']


def check_revision(horizon, args):
    pin = pinned_revision()
    head = subprocess.run(['git', '-C', str(horizon), 'rev-parse', 'HEAD'],
                          capture_output=True, text=True).stdout.strip()
    if head != pin and not args.allow_revision_mismatch:
        raise SystemExit(f'{horizon} is at {head or "an unknown commit"}, but modpack.toml pins {pin}.\n'
                         f'  git -C {horizon} fetch --depth 1 origin {pin} && git -C {horizon} checkout {pin}\n'
                         'or pass --allow-revision-mismatch.')


def toml_value(value):
    if isinstance(value, bool):
        return 'true' if value else 'false'
    if isinstance(value, str):
        return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, list):
        return '[' + ', '.join(toml_value(item) for item in value) + ']'
    if isinstance(value, dict):
        return '{ ' + ', '.join(f'{key} = {toml_value(item)}' for key, item in value.items()) + ' }'
    raise SystemExit(f'cannot write {value!r} to a manifest')


def sync_tree(source, dest, rewrite=None):
    """Make `dest` hold what `source` does, writing only the files that differ.

    A file that is already the same keeps its bytes and its modification time, so
    Cargo rebuilds exactly what changed. `rewrite` maps a file's relative path to the
    text it should hold instead (a staged manifest).
    """
    rewrite = rewrite or {}
    wanted = set()
    for path in sorted(source.rglob('*')):
        relative = path.relative_to(source)
        if not path.is_file() or 'target' in relative.parts or relative.name == 'Cargo.lock':
            continue
        wanted.add(relative)
        content = rewrite[relative].encode() if relative in rewrite else path.read_bytes()
        target = dest / relative
        if not target.is_file() or target.read_bytes() != content:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
    if dest.is_dir():
        for path in sorted(dest.rglob('*'), reverse=True):
            if path.is_file() and path.relative_to(dest) not in wanted:
                path.unlink()
            elif path.is_dir() and not any(path.iterdir()):
                path.rmdir()


def manifest_text(manifest):
    """A Cargo manifest of plain tables: all an [api] package needs."""
    lines = []
    for table, entries in manifest.items():
        lines.append(f'[{table}]')
        for key, value in entries.items():
            key = key if key.replace('-', '').replace('_', '').isalnum() else toml_value(key)
            lines.append(f'{key} = {toml_value(value)}')
        lines.append('')
    return '\n'.join(lines)


def write_if_changed(path, text):
    if not path.is_file() or path.read_text() != text:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


def stage(found, horizon):
    STAGE.mkdir(parents=True, exist_ok=True)
    members, staged = [], set()
    for mod_id, bundle, mixin in found:
        crates = {}
        for crate in mixin.get('crate', []):
            source = (bundle / crate['path']).resolve()
            relative = f'native-crates/{mod_id}/{crate["id"]}'
            sync_tree(source, STAGE / relative)
            crates[source] = relative
            members.append(relative)
            staged.add(relative)
        api = mixin.get('api', {}).get('path')
        if not api:
            continue
        source = (bundle / api).resolve()
        relative = f'native-api/{mod_id}'
        manifest = tomllib.loads((source / 'Cargo.toml').read_text())
        unknown = set(manifest) - {'package', 'dependencies', 'features', 'lib'}
        if unknown:
            raise SystemExit(f'{source}/Cargo.toml: this script does not stage [{", ".join(sorted(unknown))}]')
        dependencies = manifest.get('dependencies', {})
        if 'horizon-client' not in dependencies:
            raise SystemExit(f'{source}/Cargo.toml: missing horizon-client dependency')
        for name, dependency in dependencies.items():
            if name == 'horizon-client':
                # Whatever its source, the game's own crate, with the game's features.
                dependencies[name] = {'path': str(horizon / 'crates' / 'horizon-client'),
                                      'default-features': False}
            elif isinstance(dependency, dict) and 'path' in dependency:
                target = (source / dependency['path']).resolve()
                if target not in crates:
                    raise SystemExit(f'{source}/Cargo.toml: dependency {name} path '
                                     f'{dependency["path"]!r} is not a [[crate]] of this bundle')
                dependency['path'] = f'../../{crates[target]}'
        sync_tree(source, STAGE / relative, {Path('Cargo.toml'): manifest_text(manifest)})
        members.append(relative)
        staged.add(relative)
    # A mod or crate that is gone from the pack leaves the stage too.
    for folder in ('native-api', 'native-crates'):
        for manifest in sorted((STAGE / folder).glob('**/Cargo.toml')):
            if manifest.parent.relative_to(STAGE).as_posix() not in staged:
                shutil.rmtree(manifest.parent)
    listed = ', '.join(f'"{member}"' for member in members)
    write_if_changed(STAGE / 'Cargo.toml',
                     '# Written by tools/horizon.py: do not edit.\n'
                     f'[workspace]\nresolver = "2"\nmembers = [{listed}]\n{PROFILE}')
    # Horizon's lock file is the allow list of crates and versions a mod may use, so the
    # stage starts from it: Cargo drops what the mods don't reach and adds only the mods.
    if not (STAGE / 'Cargo.lock').exists():
        shutil.copyfile(horizon / 'Cargo.lock', STAGE / 'Cargo.lock')


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('command', choices=['stage', 'check', 'clippy'])
    parser.add_argument('mods', nargs='*', help='mod ids (default: all)')
    parser.add_argument('--horizon', help='the Horizon checkout')
    parser.add_argument('--target-dir', help='a Cargo target directory to reuse')
    parser.add_argument('--allow-revision-mismatch', action='store_true')
    args = parser.parse_args()
    horizon = horizon_checkout(args)
    check_revision(horizon, args)
    # Every bundle is staged, as the launcher builds them together; `mods` picks what to check.
    everything = bundles([])
    wanted = {mod_id for mod_id, _, _ in bundles(args.mods)}
    stage(everything, horizon)
    packages = [tomllib.loads((bundle / mixin['api']['path'] / 'Cargo.toml').read_text())['package']['name']
                for mod_id, bundle, mixin in everything if mod_id in wanted and 'api' in mixin]
    print(f'staged {len(everything)} mods in {STAGE} against {horizon}')
    if args.command == 'stage':
        return 0
    # A target directory of the stage's own by default: Cargo names its outputs by a
    # package's path inside its workspace, so two workspaces laid out alike must not
    # share one.
    target = Path(args.target_dir).resolve() if args.target_dir else ROOT / 'target' / 'horizon'
    command = ['cargo', args.command, '--manifest-path', str(STAGE / 'Cargo.toml'),
               '--target-dir', str(target)]
    for package in packages:
        command += ['-p', package]
    if args.command == 'clippy':
        command += ['--', '-D', 'warnings']
    print(' '.join(command), flush=True)
    return subprocess.run(command).returncode


if __name__ == '__main__':
    sys.exit(main())
