import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

REPO = os.environ['GITHUB_REPOSITORY']
MAXIMUM = 18446744073709551615
LEGACY_ASSETS = ('Vega-macos-arm64.zip', 'Vega-macos-arm64.zip.sha256')
ASSETS = (*LEGACY_ASSETS, 'Vega-update.json', 'Vega-update.json.sig')


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def api(path, method='GET', data=None):
    args = ['gh', 'api', f'repos/{REPO}/{path}', '--method', method]
    if data is not None:
        args += ['--input', '-']
    result = subprocess.run(args, input=json.dumps(data) if data is not None else None,
                            text=True, stdout=subprocess.PIPE, check=True)
    return json.loads(result.stdout) if result.stdout.strip() else None


def version(tag):
    match = re.fullmatch(r'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', tag)
    if not match or any(len(part) > 20 for part in match.groups()):
        return None
    result = tuple(int(part) for part in match.groups())
    return result if all(part <= MAXIMUM for part in result) else None


def ancestor(old, new):
    result = subprocess.run(['git', 'merge-base', '--is-ancestor', old, new])
    if result.returncode not in (0, 1):
        raise RuntimeError('Cannot establish release ancestry')
    return result.returncode == 0


def tags():
    return {tag: version(tag) for tag in run('git', 'tag', '--list').splitlines() if version(tag)}


def commit(tag):
    return run('git', 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}')


def releases():
    items = []
    page = 1
    while True:
        batch = api(f'releases?per_page=100&page={page}')
        items.extend(batch)
        if len(batch) < 100:
            return items
        page += 1


def validate_publication(tag, sha, items):
    if not ancestor(sha, 'origin/master'):
        raise RuntimeError('Release commit must belong to master history')
    for release in items:
        previous = version(release['tag_name'])
        if release['draft'] or release['prerelease'] or previous is None or release['tag_name'] == tag:
            continue
        if previous >= version(tag) or not ancestor(commit(release['tag_name']), sha):
            raise RuntimeError('Refusing stale or reverse-order stable publication')


def matching_release(tag, items):
    matches = [item for item in items if item['tag_name'] == tag]
    if len(matches) > 1:
        raise RuntimeError('Ambiguous releases for tag')
    return matches[0] if matches else None


def complete(release, allow_legacy=False):
    assets = {asset['name']: asset for asset in release['assets']}
    historical = allow_legacy and not run(
        'git', 'ls-tree', '--name-only', f"refs/tags/{release['tag_name']}",
        '--', 'assets/update-public-key.hex')
    required = LEGACY_ASSETS if historical else ASSETS
    return not release['prerelease'] and all(
        name in assets and assets[name]['state'] == 'uploaded' and assets[name]['size'] > 0
        for name in required)


def prepare():
    sha = run('git', 'rev-parse', 'HEAD')
    stable = tags()
    if os.environ.get('AUTOMATIC') == 'true':
        existing = [tag for tag in stable if commit(tag) == sha]
        if len(existing) > 1:
            raise RuntimeError('Multiple stable tags already identify this commit')
        if existing:
            tag = existing[0]
        elif stable:
            major, minor, patch = max(stable.values())
            if patch == MAXIMUM:
                raise RuntimeError('Patch version overflow')
            tag = f'v{major}.{minor}.{patch + 1}'
        else:
            with open('Cargo.toml', 'rb') as source:
                tag = 'v' + tomllib.load(source)['workspace']['package']['version']
    else:
        tag = os.environ.get('RELEASE_TAG', '')
        if os.environ.get('RELEASE_REF_TYPE') != 'tag':
            raise RuntimeError('Manual publication requires a tag ref')
        if tag not in stable or commit(tag) != sha:
            raise RuntimeError('Requested tag does not identify the checked-out commit')
    if version(tag) is None:
        raise RuntimeError('Release version must be canonical stable unsigned 64-bit components')
    items = releases()
    release = matching_release(tag, items)
    if release and not release['draft']:
        if tag not in stable or commit(tag) != sha or not complete(release, allow_legacy=True):
            raise RuntimeError('Published release is incomplete or mismatched; assets are immutable')
        done = True
    else:
        validate_publication(tag, sha, items)
        if tag not in stable:
            api('git/refs', 'POST', {'ref': f'refs/tags/{tag}', 'sha': sha})
        done = False
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        output.write(f'tag={tag}\ncomplete={str(done).lower()}\nsha={sha}\n')
    print(f'{tag}: ' + ('already published' if done else 'reserved for build'))


def publish():
    tag = os.environ['RELEASE_TAG']
    sha = os.environ['RELEASE_SHA']
    if version(tag) is None or run('git', 'rev-parse', 'HEAD') != sha:
        raise RuntimeError('Publication identity mismatch')
    run('git', 'fetch', 'origin', 'master', '--tags')
    if commit(tag) != sha:
        raise RuntimeError('Release tag moved since version reservation')
    items = releases()
    release = matching_release(tag, items)
    if release and not release['draft']:
        if not complete(release, allow_legacy=True):
            raise RuntimeError('Published assets cannot be repaired by overwriting')
        return
    validate_publication(tag, sha, items)
    if release is None:
        notes = api('releases/generate-notes', 'POST', {'tag_name': tag, 'target_commitish': sha})
        release = api('releases', 'POST', {'tag_name': tag, 'target_commitish': sha,
                      'name': tag, 'body': notes['body'], 'draft': True, 'prerelease': False})
    for name in ASSETS:
        path = Path('dist') / name
        if not path.is_file() or path.stat().st_size == 0:
            raise RuntimeError('Missing completed release asset')
    before_upload = api(f"releases/{release['id']}")
    if not before_upload['draft'] or before_upload['tag_name'] != tag:
        raise RuntimeError('Release changed externally; refusing to replace assets')
    subprocess.run(['gh', 'release', 'upload', tag, '--repo', REPO, '--clobber',
                    *(str(Path('dist') / name) for name in ASSETS)], check=True)
    refreshed = api(f"releases/{release['id']}")
    if not refreshed['draft'] or not complete(refreshed):
        raise RuntimeError('Draft asset upload incomplete or release changed externally')
    validate_publication(tag, sha, releases())
    api(f"releases/{release['id']}", 'PATCH', {'draft': False, 'prerelease': False, 'make_latest': 'true'})
    print(f'Published {tag}')


if __name__ == '__main__':
    if len(sys.argv) != 2 or sys.argv[1] not in ('prepare', 'publish'):
        sys.exit('Expected prepare or publish')
    try:
        {'prepare': prepare, 'publish': publish}[sys.argv[1]]()
    except (RuntimeError, subprocess.CalledProcessError, OSError, KeyError) as error:
        sys.exit(str(error))
