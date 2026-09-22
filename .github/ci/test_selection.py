#!/usr/bin/env python3
"""Issue #136 shadow report only. Never supplies a test execution filter."""
import argparse
import html
import json
from pathlib import Path, PurePosixPath
import re
import subprocess


def run(root, *args):
    return subprocess.check_output(args, cwd=root, stderr=subprocess.PIPE, timeout=900)


def read_changes(root, base, head):
    if not all(re.fullmatch(r'[0-9a-f]{40}', ref) for ref in (base, head)):
        raise ValueError('base/head must be exact commit SHAs')
    data = run(root, 'git', 'diff', '--name-status', '-z', '--find-renames', base, head, '--')
    fields = data.decode('utf-8', errors='strict').split('\0')
    if fields.pop() != '':
        raise ValueError('unterminated diff')
    changes = []
    while fields:
        status = fields.pop(0)
        count = 2 if status.startswith(('R', 'C')) else 1
        if not re.fullmatch(r'[AMDTUXB]|[RC][0-9]+', status) or len(fields) < count:
            raise ValueError('invalid diff record')
        paths, fields = fields[:count], fields[count:]
        for path in paths:
            if not path or PurePosixPath(path).is_absolute() or '..' in PurePosixPath(path).parts:
                raise ValueError('non-repository path')
        changes.append({'status': status, 'paths': paths})
    return changes


def workspace_graph(metadata):
    root = Path(metadata['workspace_root'])
    members = set(metadata['workspace_members'])
    packages = {p['id']: p for p in metadata['packages'] if p['id'] in members}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    if not members or set(packages) != members or not members <= nodes.keys():
        raise ValueError('incomplete workspace metadata')
    names = {i: p['name'] for i, p in packages.items()}
    if len(set(names.values())) != len(names):
        raise ValueError('ambiguous package names')
    directories = {}
    reverse = {name: set() for name in names.values()}
    for ident, package in packages.items():
        directory = Path(package['manifest_path']).parent.relative_to(root).as_posix()
        if directory == '.' or directory in directories:
            raise ValueError('ambiguous crate directory')
        directories[directory + '/'] = names[ident]
        for dependency in nodes[ident]['dependencies']:
            if dependency not in nodes:
                raise ValueError('incomplete dependency graph')
            if dependency in members:
                reverse[names[dependency]].add(names[ident])
    return directories, reverse


def test_inventory(inventory):
    runnable, all_tests = set(), set()
    suites = inventory['rust-suites']
    if not isinstance(suites, dict) or not suites:
        raise ValueError('empty inventory')
    for binary, suite in suites.items():
        if suite['status'] != 'listed':
            raise ValueError('unlisted test binary')
        for name, case in suite['testcases'].items():
            identity = (suite['package-name'], binary, name)
            all_tests.add(identity)
            if not isinstance(case['ignored'], bool):
                raise ValueError('invalid ignored flag')
            expected_match = {'status': 'mismatch', 'reason': 'ignored'} if case['ignored'] else {'status': 'matches'}
            if case['filter-match'] != expected_match:
                raise ValueError('filtered inventory is not a full suite')
            if not case['ignored']:
                runnable.add(identity)
    if len(all_tests) != inventory['test-count'] or not runnable:
        raise ValueError('incomplete inventory')
    return runnable


def analyze(metadata, inventory, rules, changes):
    directories, reverse = workspace_graph(metadata)
    tests = test_inventory(inventory)
    if (set(rules) != {'version', 'groups'} or type(rules['version']) is not int
            or rules['version'] != 1 or not isinstance(rules['groups'], list) or not rules['groups']):
        raise ValueError('unsupported or empty rules')
    groups, registered, ids = [], set(), set()
    for group in rules['groups']:
        required = {'id', 'protects', 'package', 'tests', 'affected_crates', 'trigger_paths'}
        if not required <= group.keys() or group.keys() - required - {'always_run'}:
            raise ValueError('unknown or missing rule fields')
        if not group['id'] or group['id'] in ids or not group['protects']:
            raise ValueError('invalid group identity')
        ids.add(group['id'])
        if not group['affected_crates'] or not set(group['affected_crates']) <= reverse.keys():
            raise ValueError('unknown trigger crate')
        if not isinstance(group.get('always_run', False), bool):
            raise ValueError('invalid always_run')
        if not isinstance(group['trigger_paths'], list):
            raise ValueError('invalid trigger paths')
        for path in group['trigger_paths']:
            if not isinstance(path, str) or not path or path.startswith('/') or '..' in PurePosixPath(path).parts:
                raise ValueError('invalid trigger path')
        matches = set()
        if not isinstance(group['tests'], list) or not group['tests']:
            raise ValueError('empty test allowlist')
        for name in group['tests']:
            found = {t for t in tests if t[0] == group['package'] and t[2] == name}
            if len(found) != 1 or found & (registered | matches):
                raise ValueError('stale, ambiguous or duplicate test identity')
            matches.update(found)
        registered.update(matches)
        groups.append((group, matches))
    fallback, changed = [], set()
    if not changes:
        fallback.append('empty diff: select full conservatively')
    paths = [path for change in changes for path in change['paths']]
    for change in changes:
        if change['status'] not in ('A', 'M'):
            fallback.append('delete/rename or unsupported status: both paths recorded; select full')
    for path in paths:
        # Documentation is the only non-crate category proven independent here.
        if path.startswith('docs/') and path.endswith('.md') or path == 'README.md':
            continue
        if (PurePosixPath(path).name in ('Cargo.toml', 'Cargo.lock', 'build.rs')
                or not path.endswith('.rs')):
            fallback.append('configuration or unclassified path: ' + path)
            continue
        owners = [name for prefix, name in directories.items() if path.startswith(prefix)]
        if len(owners) != 1:
            fallback.append('unknown crate path: ' + path)
        else:
            changed.add(owners[0])
    affected = set(changed)
    pending = list(changed)
    while pending:
        for caller in reverse[pending.pop()] - affected:
            affected.add(caller)
            pending.append(caller)
    result_groups, skipped = [], set()
    for group, matches in groups:
        reasons = []
        if fallback:
            reasons.append('full fallback applies')
        if group.get('always_run', False):
            reasons.append('representative: always run')
        overlap = sorted(affected & set(group['affected_crates']))
        if overlap:
            reasons.append('affected crates: ' + ', '.join(overlap))
        explicit = [p for p in paths if any(p == trigger or (trigger.endswith('/') and p.startswith(trigger))
                                          for trigger in group['trigger_paths'])]
        if explicit:
            reasons.append('explicit caller paths: ' + ', '.join(explicit))
        would_run = bool(reasons)
        if not would_run:
            skipped.update(matches)
            reasons.append('no registered dependency or caller trigger changed')
        result_groups.append({'id': group['id'], 'protects': group['protects'],
                              'decision': 'run' if would_run else 'candidate_skip', 'reasons': reasons,
                              'tests': [dict(zip(('package', 'binary', 'test'), t)) for t in sorted(matches)]})
    return {'schema_version': 1, 'mode': 'shadow', 'actual_execution': 'full',
            'fallback_reasons': list(dict.fromkeys(fallback)), 'changes': changes,
            'changed_crates': sorted(changed), 'affected_crates': sorted(affected),
            'runnable_count': len(tests), 'would_run_count': len(tests - skipped),
            'candidate_skip_count': len(skipped), 'unlisted_default_run_count': len(tests - registered),
            'groups': result_groups}


def safe(value):
    # HTML escaping plus JSON quoting makes newlines/backticks in filenames inert.
    text = html.escape(json.dumps(value, ensure_ascii=True))
    for char in '`[]*_':
        text = text.replace(char, f'&#{ord(char)};')
    return '<code>' + text + '</code>'


def summary(report):
    lines = ['## Test selection shadow report', '',
             '**Actual execution: full suite, four shards. No tests are skipped by this report.**', '',
             'Base: ' + safe(report.get('base')), 'Head: ' + safe(report.get('head')), '',
             'Runnable: ' + safe(report.get('runnable_count')) + '; would run: ' + safe(report.get('would_run_count'))
             + '; candidate skip: ' + safe(report['candidate_skip_count']), '']
    for reason in report['fallback_reasons']:
        lines.append('- Full fallback: ' + safe(reason))
    for change in report.get('changes', []):
        lines.append('- Changed: ' + safe(change))
    for group in report.get('groups', []):
        lines.append('- ' + safe(group['id']) + ': ' + safe(group['decision']) + ' — ' + safe(group['reasons']))
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', required=True)
    parser.add_argument('--head', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    report = {'schema_version': 1, 'mode': 'shadow', 'actual_execution': 'full',
              'candidate_skip_count': 0, 'fallback_reasons': [], 'changes': []}
    stage = 'diff'
    try:
        changes = read_changes(root, args.base, args.head)
        report['changes'] = changes
        stage = 'rules'
        rules = json.loads(Path(__file__).with_name('slow-tests.json').read_text())
        stage = 'metadata'
        metadata = json.loads(run(root, 'cargo', 'metadata', '--format-version', '1', '--locked'))
        stage = 'inventory'
        inventory = json.loads(run(root, 'cargo', 'nextest', 'list', '--workspace', '--message-format', 'json'))
        stage = 'analysis'
        report = analyze(metadata, inventory, rules, changes)
    except Exception as error:
        # Child stderr/metadata contains absolute paths; never publish it.
        report['fallback_reasons'] = [f'{stage} unavailable or invalid ({type(error).__name__}); select full']
    report.update(base=args.base, head=args.head)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=True) + '\n')
    args.output.with_suffix('.md').write_text(summary(report))
    print(summary(report))


if __name__ == '__main__':
    main()
