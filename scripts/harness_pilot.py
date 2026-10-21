import argparse
from contextlib import contextmanager, nullcontext
import hashlib
import json
import math
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import threading
import time


SCHEMA_VERSION = 1
COST_UNITS_PER_USD = 1000000
UPSTREAM_REVISION = '1cfe254e8d25b15f37048c3730134686abe60335'
DATA_ROOT = Path(__file__).resolve().parents[1] / 'research' / 'harness2'
DEFAULT_MODEL = {'id': 'gpt-6.1-sol', 'reasoning_effort': 'max'}
DEFAULT_LIMITS = {'timeout_ms': 300000, 'turn_limit': 24,
                  'max_output_tokens': 8192, 'max_total_tokens': 64000}
FIXED_POLICY = ('Use the native Read, Write, Edit and approved skill tools. '
                'Read relevant inputs before editing. Only declared output paths may change. '
                'Project instructions and skill contents are fixed. No shell, external '
                'directories, network tools, delegation, hooks or interactive approvals.')
TERMINALS = frozenset({'end', 'length', 'turn_limit', 'cancelled', 'timeout',
                       'failed', 'budget_exhausted'})
DOMAINS = frozenset({'retrieval', 'file_edit', 'recovery', 'skills'})
LIMIT_KEYS = frozenset(DEFAULT_LIMITS)
HASH_PATTERN = re.compile(r'[0-9a-f]{64}')
SKILL_PATTERN = re.compile(r'skills/([a-z0-9][a-z0-9_-]*)/SKILL\.md')


class StudyCancelled(BaseException):
    pass


def _cancellation_scope(launcher):
    scope = getattr(launcher, 'cancellation_scope', None)
    return scope() if callable(scope) else nullcontext()


def _cancel_requested(launcher):
    return getattr(launcher, 'cancel_requested', False) is True


def canonical_json(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'),
                      ensure_ascii=False, allow_nan=False)


def digest(value):
    text = value if isinstance(value, str) else canonical_json(value)
    return hashlib.sha256(text.encode('utf-8')).hexdigest()


def _bytes_digest(value):
    return hashlib.sha256(value).hexdigest()


def _read_json(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError('A regular JSON file is required')
    try:
        return json.loads(path.read_text(encoding='utf-8'),
                          parse_constant=lambda value: _invalid('Non-finite JSON number'))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ValueError('Invalid JSON input') from error


def _invalid(message):
    raise ValueError(message)


def _write_json(path, document):
    path = Path(path)
    temporary = path.with_name(path.name + '.pending')
    with temporary.open('x', encoding='utf-8') as output:
        output.write(canonical_json(document) + '\n')
    os.replace(temporary, path)


def _relative(path):
    if (not isinstance(path, str) or not path or '\\' in path or ':' in path
            or path.startswith('/') or any(ord(character) < 32 for character in path)
            or any(part in ('', '.', '..') for part in path.split('/'))):
        raise ValueError('Invalid relative path')
    return path


def _task_path(path):
    path = _relative(path)
    if (path.split('/')[0].lower() in ('.agents', '.git', '.vega', 'skills')
            or path.lower() in ('agents.md', 'task.json')):
        raise ValueError('Task path overlaps fixed instructions or control metadata')
    return path


def _regular_child(root, relative):
    current = Path(root)
    for component in _relative(relative).split('/'):
        current = current / component
        if current.is_symlink():
            raise ValueError('Symbolic links are not accepted')
    if current.exists() and not stat.S_ISREG(current.stat().st_mode):
        raise ValueError('Artifact is not a regular file')
    return current


def _positive_limits(limits):
    if not isinstance(limits, dict) or set(limits) != LIMIT_KEYS:
        raise ValueError('Explicit deadline, turn, output and total-token limits are required')
    if any(type(value) is not int or value <= 0 for value in limits.values()):
        raise ValueError('All execution limits must be positive integers')
    return dict(limits)


def _model(model):
    if model != DEFAULT_MODEL:
        raise ValueError('This pilot fixes gpt-6.1-sol with max reasoning effort')
    return dict(model)


def _validate_task(task):
    required = {'task_id', 'domain', 'split', 'source', 'task_prompt', 'seed_files',
                'writable_paths', 'oracle'}
    if not isinstance(task, dict) or set(task) != required:
        raise ValueError('Invalid task fields')
    if not isinstance(task['task_id'], str) or not re.fullmatch(r'[a-z0-9][a-z0-9-]*', task['task_id']):
        raise ValueError('Invalid task identifier')
    if task['domain'] not in DOMAINS or task['split'] not in ('optimization', 'holdout'):
        raise ValueError('Invalid task domain or split')
    if not isinstance(task['source'], str) or not task['source'].strip():
        raise ValueError('A task source version is required')
    if not isinstance(task['task_prompt'], str) or not task['task_prompt'].strip():
        raise ValueError('A public task prompt is required')
    seeds = task['seed_files']
    writable = task['writable_paths']
    if not isinstance(seeds, dict) or any(not isinstance(value, str) for value in seeds.values()):
        raise ValueError('Seed files must contain UTF-8 text')
    if (not isinstance(writable, list) or not writable
            or any(not isinstance(value, str) for value in writable)
            or len(set(writable)) != len(writable)):
        raise ValueError('Distinct explicit output paths are required')
    for relative in [*seeds, *writable]:
        _task_path(relative)
    paths = [*seeds, *writable]
    if len({path.casefold() for path in paths}) != len(paths):
        raise ValueError('Seed and output paths must be disjoint without case aliases')
    for path in paths:
        if any(other.startswith(path + '/') for other in paths):
            raise ValueError('Task paths overlap a directory boundary')
    oracle = task['oracle']
    if not isinstance(oracle, dict) or not isinstance(oracle.get('artifacts'), dict):
        raise ValueError('An independent artifact oracle is required')
    if set(oracle['artifacts']) != set(writable):
        raise ValueError('Every declared output requires an oracle')
    for rule in oracle['artifacts'].values():
        if not isinstance(rule, dict) or set(rule) != {'kind', 'value'}:
            raise ValueError('Invalid artifact oracle')
        if rule['kind'] not in ('text', 'json'):
            raise ValueError('Unsupported artifact oracle')
        if rule['kind'] == 'text' and not isinstance(rule['value'], str):
            raise ValueError('Text oracle requires exact text')
        canonical_json(rule['value'])
    if (not isinstance(oracle.get('required_skills'), list)
            or any(not isinstance(name, str) or not re.fullmatch(r'[a-z0-9][a-z0-9_-]*', name)
                   for name in oracle['required_skills'])):
        raise ValueError('Invalid required skill names')


def load_catalog(path):
    catalog = _read_json(path)
    if (not isinstance(catalog, dict)
            or set(catalog) != {'schema_version', 'dataset_version', 'tasks'}
            or catalog['schema_version'] != SCHEMA_VERSION
            or catalog['dataset_version'] != 'vega-microtasks-v1'
            or not isinstance(catalog['tasks'], list) or len(catalog['tasks']) != 24):
        raise ValueError('The versioned 24-task catalog is required')
    identifiers = set()
    balance = {}
    for task in catalog['tasks']:
        _validate_task(task)
        if task['task_id'] in identifiers:
            raise ValueError('Tasks cannot overlap across splits')
        identifiers.add(task['task_id'])
        key = (task['domain'], task['split'])
        balance[key] = balance.get(key, 0) + 1
    for domain in DOMAINS:
        if balance.get((domain, 'optimization')) != 4 or balance.get((domain, 'holdout')) != 2:
            raise ValueError('Each domain needs four optimization and two holdout tasks')
    return catalog


def public_task(task):
    return {key: task[key] for key in
            ('task_id', 'task_prompt', 'seed_files', 'writable_paths')}


def stage_public_tasks(catalog, destination, split):
    if split not in ('optimization', 'holdout'):
        raise ValueError('Stage one explicit split at a time')
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=False)
    staged = {}
    for task in catalog['tasks']:
        if task['split'] != split:
            continue
        _validate_task(task)
        directory = destination / task['task_id']
        directory.mkdir()
        _write_json(directory / 'task.json', public_task(task))
        for relative, text in task['seed_files'].items():
            path = directory / _task_path(relative)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding='utf-8')
        staged[task['task_id']] = directory
    return staged


def validate_tree(tree):
    if not isinstance(tree, dict) or not tree.get('strategy.md', '').strip():
        raise ValueError('A nonempty strategy.md is required')
    if sum(len(value) for value in tree.values() if isinstance(value, str)) > 100000:
        raise ValueError('Guidance exceeds the pilot surface limit')
    for relative, text in tree.items():
        _relative(relative)
        if not isinstance(text, str):
            raise ValueError('Guidance files must be UTF-8 text')
        if relative == 'strategy.md':
            continue
        match = SKILL_PATTERN.fullmatch(relative)
        if not match:
            raise ValueError('Only strategy.md and skills/<slug>/SKILL.md are editable')
        frontmatter = re.match(r'\A---\r?\n(.*?)\r?\n---(?:\r?\n|\Z)', text, re.S)
        if not frontmatter:
            raise ValueError('Skill frontmatter is required')
        name = re.search(r'^name:\s*([^\r\n]+)', frontmatter.group(1), re.M)
        description = re.search(r'^description:\s*\S[^\r\n]*', frontmatter.group(1), re.M)
        if not name or name.group(1).strip() != match.group(1) or not description:
            raise ValueError('Skill name and description must match the native catalog contract')


def load_harness(directory):
    directory = Path(directory)
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError('A regular harness directory is required')
    tree = {}
    for path in sorted(directory.rglob('*')):
        if path.is_symlink():
            raise ValueError('Symbolic links are forbidden in candidate trees')
        if path.is_dir():
            continue
        if not stat.S_ISREG(path.stat().st_mode):
            raise ValueError('Special files are forbidden in candidate trees')
        try:
            tree[path.relative_to(directory).as_posix()] = path.read_bytes().decode('utf-8')
        except UnicodeError as error:
            raise ValueError('Guidance files must be UTF-8 text') from error
    validate_tree(tree)
    return tree


def _harness_payload(tree, trusted_skill_approvals):
    validate_tree(tree)
    if not isinstance(trusted_skill_approvals, dict):
        raise ValueError('Trusted skill approvals must come from the operator')
    skills = {}
    for relative, text in tree.items():
        match = SKILL_PATTERN.fullmatch(relative)
        if match:
            name = match.group(1)
            if trusted_skill_approvals.get(name) != digest(text):
                raise ValueError('Missing or stale trusted approval for skill ' + name)
            skills[name] = text
    if any(not isinstance(value, str) or not HASH_PATTERN.fullmatch(value)
           for value in trusted_skill_approvals.values()):
        raise ValueError('Trusted skill approval hashes must be exact SHA256 values')
    return {'strategy': tree['strategy.md'], 'skills': skills}


def make_request(task, harness_dir, model, limits, fixed_policy, trusted_skill_approvals):
    _validate_task(task)
    if not isinstance(fixed_policy, str) or not fixed_policy.strip():
        raise ValueError('Fixed policy is required')
    return {'schema_version': SCHEMA_VERSION, **public_task(task),
            'fixed_policy': fixed_policy,
            'harness': _harness_payload(load_harness(harness_dir), trusted_skill_approvals),
            'model': _model(model), 'limits': _positive_limits(limits),
            'trusted_skill_approvals': dict(trusted_skill_approvals)}


def _prepare_launcher(launcher, request):
    prepare = getattr(launcher, 'prepare', None)
    if callable(prepare):
        prepare(request)


def launcher_identity(launcher):
    identity = getattr(launcher, 'config_identity', None)
    if not isinstance(identity, str) or not HASH_PATTERN.fullmatch(identity):
        raise ValueError('An explicit validated provider configuration identity is required')
    driver = getattr(launcher, 'driver_identity', None)
    return digest({'config': identity, 'driver': driver}) if driver is not None else identity


def _verify_native(native, request, run_id, launcher=None):
    if (not isinstance(native, dict) or native.get('schema_version') != SCHEMA_VERSION
            or native.get('backend') != 'vega_native_runtime'
            or native.get('task_id') != request['task_id'] or native.get('run_id') != run_id
            or native.get('terminal') not in TERMINALS):
        raise ValueError('Native result identity or terminal is invalid')
    hashes = native.get('hashes')
    if not isinstance(hashes, dict):
        raise ValueError('Native provenance hashes are missing')
    expected = {'task': digest({key: request[key] for key in ('task_id', 'task_prompt')}),
                'seed': digest(request['seed_files']), 'policy': digest(request['fixed_policy']),
                'harness': digest(request['harness']), 'model': digest(request['model']),
                'request': digest(request)}
    if any(hashes.get(key) != value for key, value in expected.items()):
        raise ValueError('Native task, model, limits or guidance does not match the request')
    for key in ('instructions', 'config'):
        if not isinstance(hashes.get(key), str) or not HASH_PATTERN.fullmatch(hashes[key]):
            raise ValueError('Native fixed configuration provenance is missing')
    if launcher is not None and getattr(launcher, 'native_config_hash', hashes['config']) != hashes['config']:
        raise ValueError('Native provider configuration changed after preflight')
    if not isinstance(native.get('metrics'), dict) or not isinstance(native.get('trace'), list):
        raise ValueError('Native metrics and trace are required')
    for event in native['trace']:
        if isinstance(event, dict) and event.get('event') == 'skill_activation' and event.get('status') == 'loaded':
            name = event.get('name')
            body = request['harness']['skills'].get(name)
            if (not isinstance(body, str) or event.get('content_sha256') != digest(body)
                    or request['trusted_skill_approvals'].get(name) != digest(body)):
                raise ValueError('Loaded skill does not match its trusted approved content')
    if type(native.get('wall_time_ms')) is not int or native['wall_time_ms'] < 0:
        raise ValueError('Native wall time is invalid')


def _verify_artifacts(native, request, trial_root):
    trial_root = Path(trial_root)
    if trial_root.is_symlink() or not trial_root.is_dir():
        raise ValueError('Native trial root is missing or symlinked')
    workspace = trial_root / 'workspace'
    if workspace.is_symlink() or not workspace.is_dir():
        raise ValueError('Native workspace is missing')
    artifacts = native.get('artifacts')
    if not isinstance(artifacts, dict) or set(artifacts) - set(request['writable_paths']):
        raise ValueError('Native result includes undeclared artifacts')
    for relative, expected in request['seed_files'].items():
        path = _regular_child(workspace, relative)
        if not path.is_file() or path.read_bytes().decode('utf-8') != expected:
            raise ValueError('Native execution changed a fixed seed input')
    for relative in request['writable_paths']:
        path = _regular_child(workspace, relative)
        if path.is_file():
            actual = path.read_bytes().decode('utf-8')
            if artifacts.get(relative) != actual:
                raise ValueError('Native artifact metadata disagrees with actual bytes')
        elif relative in artifacts:
            raise ValueError('Native result claims a missing artifact')
    instructions = workspace / 'AGENTS.md'
    if instructions.exists():
        path = _regular_child(workspace, 'AGENTS.md')
        if digest(path.read_bytes().decode('utf-8')) != native['hashes']['instructions']:
            raise ValueError('Fixed project instructions changed')


def evaluate_result(task, native_result):
    errors = []
    if native_result.get('backend') != 'vega_native_runtime':
        errors.append('native_backend_missing')
    if native_result.get('task_id') != task['task_id']:
        errors.append('task_identity_mismatch')
    if native_result.get('terminal') != 'end':
        errors.append('execution_not_naturally_finished')
    artifacts = native_result.get('artifacts', {})
    if not isinstance(artifacts, dict):
        artifacts = {}
    for relative, rule in task['oracle']['artifacts'].items():
        actual = artifacts.get(relative)
        if not isinstance(actual, str):
            errors.append('missing_artifact:' + relative)
            continue
        if rule['kind'] == 'text':
            correct = actual == rule['value']
        else:
            try:
                parsed = json.loads(actual, parse_constant=lambda value: _invalid('Non-finite artifact number'))
                correct = canonical_json(parsed) == canonical_json(rule['value'])
            except (ValueError, TypeError):
                correct = False
        if not correct:
            errors.append('artifact_mismatch:' + relative)
    for name in task['oracle']['required_skills']:
        activated = any(isinstance(event, dict) and event.get('event') == 'skill_activation'
                        and event.get('name') == name and event.get('status') == 'loaded'
                        for event in native_result.get('trace', []))
        if not activated:
            errors.append('skill_not_loaded:' + name)
    return {'delivered': not errors, 'errors': errors}


def read_run(run_dir):
    run_dir = Path(run_dir)
    if run_dir.is_symlink() or not run_dir.is_dir():
        raise ValueError('Run directory is missing')
    manifest = _read_json(run_dir / 'manifest.json')
    if manifest.get('schema_version') != SCHEMA_VERSION or manifest.get('state') != 'complete':
        raise ValueError('Run is incomplete and will not be replayed')
    inputs = _read_json(run_dir / 'input.json')
    native = _read_json(run_dir / 'native.json')
    if (digest(inputs) != manifest.get('input_sha256')
            or digest(native) != manifest.get('native_sha256')):
        raise ValueError('Persisted run metadata changed')
    _verify_native(native, inputs['request'], run_dir.name)
    _verify_artifacts(native, inputs['request'], run_dir / 'native')
    return {'native': native, 'evaluation': evaluate_result(inputs['task'], native),
            'input_identity': manifest['input_sha256'],
            'provider_identity': inputs['provider_identity'],
            'execution_kind': inputs['execution_kind']}


def run_trial(*, task, harness_dir, runs_root, run_id, model, limits, fixed_policy,
              trusted_skill_approvals, launcher):
    with _cancellation_scope(launcher):
        return _run_trial(task=task, harness_dir=harness_dir, runs_root=runs_root,
                          run_id=run_id, model=model, limits=limits, fixed_policy=fixed_policy,
                          trusted_skill_approvals=trusted_skill_approvals, launcher=launcher)


def _run_trial(*, task, harness_dir, runs_root, run_id, model, limits, fixed_policy,
               trusted_skill_approvals, launcher):
    if (not isinstance(run_id, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,127}', run_id)
            or run_id in ('.', '..')):
        raise ValueError('run_id must be a safe single path component')
    request = make_request(task, harness_dir, model, limits, fixed_policy, trusted_skill_approvals)
    _prepare_launcher(launcher, request)
    inputs = {'schema_version': SCHEMA_VERSION, 'task': task, 'request': request,
              'provider_identity': launcher_identity(launcher),
              'execution_kind': getattr(launcher, 'execution_kind', 'injected_boundary')}
    run_dir = Path(runs_root) / run_id
    if run_dir.exists() or run_dir.is_symlink():
        existing = read_run(run_dir)
        if existing['input_identity'] != digest(inputs):
            raise ValueError('run_id already belongs to different inputs or provider configuration')
        return run_dir
    run_dir.parent.mkdir(parents=True, exist_ok=True)
    run_dir.mkdir(mode=0o700)
    _write_json(run_dir / 'input.json', inputs)
    _write_json(run_dir / 'manifest.json', {'schema_version': SCHEMA_VERSION, 'state': 'incomplete',
                                         'input_sha256': digest(inputs)})
    native = launcher(request, run_dir / 'native', run_id)
    _verify_native(native, request, run_id, launcher)
    _verify_artifacts(native, request, run_dir / 'native')
    _write_json(run_dir / 'native.json', native)
    _write_json(run_dir / 'evaluation.json', evaluate_result(task, native))
    _write_json(run_dir / 'manifest.json', {'schema_version': SCHEMA_VERSION, 'state': 'complete',
                                         'input_sha256': digest(inputs), 'native_sha256': digest(native)})
    return run_dir


def _known_cost(metrics):
    amount = metrics.get('cost_microcents')
    return (type(amount) is int and amount >= 0 and metrics.get('pricing_known') is True
            and metrics.get('usage_complete', True) is True)


def aggregate_results(records, search_ledger=None):
    successes = sum(record.get('evaluation', {}).get('delivered') is True for record in records)
    metrics = [(record.get('native') or {}).get('metrics', {}) for record in records]
    known_costs = [metric['cost_microcents'] for metric in metrics if _known_cost(metric)]
    unknown = len(records) - len(known_costs)
    cost = sum(known_costs) / COST_UNITS_PER_USD if not unknown else None
    search_entries = search_ledger.get('entries', []) if isinstance(search_ledger, dict) else []
    search_known = [entry['cost_microcents'] for entry in search_entries if _known_cost(entry)]
    search_complete = (isinstance(search_ledger, dict) and search_ledger.get('complete') is True
                       and len(search_known) == len(search_entries))
    search_cost = sum(search_known) / COST_UNITS_PER_USD if search_complete else None
    terminals = {}
    for record in records:
        terminal = (record.get('native') or {}).get('terminal', 'controller_failure')
        terminals[terminal] = terminals.get(terminal, 0) + 1
    result = {'trials': len(records), 'successes': successes,
              'success_rate': successes / len(records) if records else None,
              'wall_time_ms': sum((record.get('native') or {}).get('wall_time_ms',
                                                                  record.get('wall_time_ms', 0))
                                  for record in records),
              'cost_units_per_usd': COST_UNITS_PER_USD,
              'cost_usd': cost, 'known_cost_usd': sum(known_costs) / COST_UNITS_PER_USD,
              'unknown_cost_trials': unknown, 'search_cost_usd': search_cost,
              'known_search_cost_usd': sum(search_known) / COST_UNITS_PER_USD,
              'total_cost_usd': cost + search_cost if cost is not None and search_cost is not None else None,
              'cost_per_success_usd': cost / successes if cost is not None and successes else None,
              'terminal_counts': terminals}
    for key in ('primary_model_requests', 'tool_proposals', 'tool_executions',
                'rejected_tool_calls', 'repeated_tool_proposals', 'input_tokens', 'output_tokens'):
        values = [metric.get(key) for metric in metrics]
        result[key] = sum(values) if all(type(value) is int and value >= 0 for value in values) else None
    return result


def _directory_hash(directory):
    directory = Path(directory)
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError('A regular frozen directory is required')
    inventory = {}
    for path in sorted(directory.rglob('*')):
        if path.is_symlink():
            raise ValueError('Frozen data cannot contain symbolic links')
        if path.is_file():
            inventory[path.relative_to(directory).as_posix()] = _bytes_digest(path.read_bytes())
        elif not path.is_dir():
            raise ValueError('Frozen data cannot contain special files')
    return digest(inventory)


def freeze_plan(*, catalog_path, baseline_dir, frozen_dir, library_dir, model, limits,
                fixed_policy, trusted_skill_approvals, provider_identity, repetitions=3):
    if type(repetitions) is not int or repetitions <= 0:
        raise ValueError('Both arms need the same positive number of repetitions')
    if not isinstance(provider_identity, str) or not HASH_PATTERN.fullmatch(provider_identity):
        raise ValueError('An explicit provider identity is required to freeze execution')
    if not isinstance(fixed_policy, str) or not fixed_policy.strip():
        raise ValueError('A nonempty fixed policy is required')
    catalog = load_catalog(catalog_path)
    baseline = load_harness(baseline_dir)
    frozen = load_harness(frozen_dir)
    _harness_payload(baseline, trusted_skill_approvals)
    _harness_payload(frozen, trusted_skill_approvals)
    identity = {'catalog': digest(catalog), 'dataset_version': catalog['dataset_version'],
                'baseline': digest(baseline), 'frozen': digest(frozen),
                'library': _directory_hash(library_dir), 'model': _model(model),
                'limits': _positive_limits(limits), 'fixed_policy': fixed_policy,
                'trusted_skill_approvals': dict(trusted_skill_approvals),
                'provider_identity': provider_identity, 'repetitions': repetitions,
                'upstream_revision': UPSTREAM_REVISION}
    tasks = [task['task_id'] for task in catalog['tasks'] if task['split'] == 'holdout']
    return {'schema_version': SCHEMA_VERSION, 'status': 'planned', 'plan_id': digest(identity),
            'planned_trials': len(tasks) * repetitions * 2, 'tasks': tasks,
            'arms': ['baseline', 'frozen'], 'identity': identity,
            'sources': {key: str(Path(value).resolve()) for key, value in {
                'catalog_path': catalog_path, 'baseline_dir': baseline_dir,
                'frozen_dir': frozen_dir, 'library_dir': library_dir}.items()}}


def run_holdout(*, plan, catalog_path, baseline_dir, frozen_dir, library_dir, model, limits,
                fixed_policy, trusted_skill_approvals, provider_identity, runs_root,
                launcher, budget_usd=None):
    with _cancellation_scope(launcher):
        return _run_holdout(plan=plan, catalog_path=catalog_path, baseline_dir=baseline_dir,
                            frozen_dir=frozen_dir, library_dir=library_dir, model=model, limits=limits,
                            fixed_policy=fixed_policy, trusted_skill_approvals=trusted_skill_approvals,
                            provider_identity=provider_identity, runs_root=runs_root,
                            launcher=launcher, budget_usd=budget_usd)


def _run_holdout(*, plan, catalog_path, baseline_dir, frozen_dir, library_dir, model, limits,
                 fixed_policy, trusted_skill_approvals, provider_identity, runs_root,
                 launcher, budget_usd=None):
    if (plan.get('schema_version') != SCHEMA_VERSION or plan.get('status') != 'planned'
            or plan.get('arms') != ['baseline', 'frozen']):
        raise ValueError('An intact planned two-arm holdout manifest is required')
    if budget_usd is not None:
        _positive_budget(budget_usd)
    arguments = dict(catalog_path=catalog_path, baseline_dir=baseline_dir, frozen_dir=frozen_dir,
                     library_dir=library_dir, model=model, limits=limits, fixed_policy=fixed_policy,
                     trusted_skill_approvals=trusted_skill_approvals, provider_identity=provider_identity,
                     repetitions=plan['identity']['repetitions'])

    def verify_frozen():
        current = freeze_plan(**arguments)
        if (current['identity'] != plan['identity'] or current['plan_id'] != plan.get('plan_id')
                or current['tasks'] != plan.get('tasks')
                or current['planned_trials'] != plan.get('planned_trials')):
            raise ValueError('Frozen tree, library, model, limits, provider or dataset drifted')

    verify_frozen()
    catalog = load_catalog(catalog_path)
    tasks = [task for task in catalog['tasks'] if task['split'] == 'holdout']
    records = []
    paired = []
    config_hash = None
    started = time.monotonic()
    status = 'completed'
    try:
        _prepare_launcher(launcher, make_request(tasks[0], baseline_dir, model, limits,
                                               fixed_policy, trusted_skill_approvals))
        if launcher_identity(launcher) != provider_identity:
            raise ValueError('Launcher provider or binary differs from the frozen plan')
    except StudyCancelled:
        status = 'cancelled'
    for task_index, task in enumerate(tasks):
        for repeat in range(plan['identity']['repetitions']):
            arms = ('baseline', 'frozen') if (task_index + repeat) % 2 == 0 else ('frozen', 'baseline')
            for arm in arms:
                if status == 'cancelled' or _cancel_requested(launcher):
                    status = 'cancelled'
                    break
                verify_frozen()
                identifier = 'holdout-' + digest([plan['plan_id'], task['task_id'], arm, repeat])[:48]
                before = time.monotonic()
                try:
                    directory = run_trial(task=task, harness_dir=baseline_dir if arm == 'baseline' else frozen_dir,
                                          runs_root=runs_root, run_id=identifier, model=model, limits=limits,
                                          fixed_policy=fixed_policy, trusted_skill_approvals=trusted_skill_approvals,
                                          launcher=launcher)
                    record = read_run(directory)
                    observed_config = record['native']['hashes']['config']
                    if config_hash is not None and observed_config != config_hash:
                        raise ValueError('Mixed native provider or tool configurations cannot be compared')
                    config_hash = observed_config
                except StudyCancelled:
                    if (Path(runs_root) / identifier).is_dir():
                        records.append({'task_id': task['task_id'], 'arm': arm, 'repeat': repeat,
                                        'run_id': identifier, 'native': None,
                                        'evaluation': {'delivered': False,
                                                       'errors': ['cancelled_without_native_result']},
                                        'wall_time_ms': int((time.monotonic() - before) * 1000),
                                        'execution_kind': getattr(launcher, 'execution_kind', 'injected_boundary')})
                    status = 'cancelled'
                    break
                except (OSError, RuntimeError) as error:
                    record = {'native': None, 'evaluation': {'delivered': False,
                              'errors': ['native_launch_failed']}, 'failure_category': type(error).__name__,
                              'wall_time_ms': int((time.monotonic() - before) * 1000),
                              'execution_kind': getattr(launcher, 'execution_kind', 'injected_boundary')}
                verify_frozen()
                records.append({'task_id': task['task_id'], 'arm': arm, 'repeat': repeat,
                                'run_id': identifier, **record})
                if _cancel_requested(launcher) or (record.get('native') or {}).get('terminal') == 'cancelled':
                    status = 'cancelled'
                    break
                if budget_usd is not None:
                    used = aggregate_results(records)
                    if used['cost_usd'] is None or used['cost_usd'] >= budget_usd:
                        status = 'cost_unknown' if used['cost_usd'] is None else 'budget_exhausted'
                        break
            if status != 'completed':
                break
            pair = {record['arm']: record['evaluation']['delivered'] for record in records
                    if record['task_id'] == task['task_id'] and record['repeat'] == repeat}
            paired.append({'task_id': task['task_id'], 'repeat': repeat, **pair})
        if status != 'completed':
            break
    verify_frozen()
    if _cancel_requested(launcher):
        status = 'cancelled'
    return {'schema_version': SCHEMA_VERSION, 'status': status, 'plan_id': plan['plan_id'],
            'planned_trials': plan['planned_trials'], 'trials': records, 'paired': paired,
            'evidence': getattr(launcher, 'execution_kind', 'injected_boundary'),
            'end_to_end_wall_time_ms': int((time.monotonic() - started) * 1000),
            'budget_usd': budget_usd, 'currency_cap_is_hard': False,
            'comparison': {arm: aggregate_results([record for record in records if record['arm'] == arm],
                                                 search_ledger={'complete': True, 'entries': []})
                           for arm in ('baseline', 'frozen')},
            'identity': plan['identity']}


class NativeProcessLauncher:
    execution_kind = 'native_process'

    def __init__(self, binary, provider_config, secret=None):
        self.binary = Path(binary).resolve()
        self.provider_config = Path(provider_config).resolve()
        if not self.binary.is_file() or not os.access(self.binary, os.X_OK):
            raise ValueError('Provide an existing executable native harness_eval binary')
        config = _read_json(self.provider_config)
        self._reject_credentials(config)
        self.driver_identity = {'binary_sha256': _bytes_digest(self.binary.read_bytes()),
                                'provider_config_sha256': digest(config)}
        self.config_identity = None
        self.native_config_hash = None
        self.preflight = None
        self._secret = secret
        self._cancel_signal = None
        self._signal_scope_depth = 0

    @property
    def cancel_requested(self):
        return self._cancel_signal is not None

    def request_cancel(self, signum=signal.SIGINT):
        if self._cancel_signal is None:
            self._cancel_signal = signum

    def raise_if_cancelled(self):
        if self.cancel_requested:
            raise StudyCancelled()

    @contextmanager
    def cancellation_scope(self):
        if threading.current_thread() is not threading.main_thread():
            yield
            return
        self._signal_scope_depth += 1
        previous = {}

        def request(signum, frame):
            self.request_cancel(signum)

        try:
            if self._signal_scope_depth == 1:
                for signum in (signal.SIGINT, signal.SIGTERM):
                    previous[signum] = signal.getsignal(signum)
                    signal.signal(signum, request)
            yield
        finally:
            for signum, handler in previous.items():
                if signal.getsignal(signum) is request:
                    signal.signal(signum, handler)
            self._signal_scope_depth -= 1

    @staticmethod
    def _reject_credentials(value):
        if isinstance(value, dict):
            for key, child in value.items():
                if re.sub(r'[^a-z]', '', key.lower()) in ('apikey', 'token', 'authorization', 'secret'):
                    raise ValueError('Provider configuration must not contain credentials')
                NativeProcessLauncher._reject_credentials(child)
        elif isinstance(value, list):
            for child in value:
                NativeProcessLauncher._reject_credentials(child)

    def _check_driver(self):
        current = {'binary_sha256': _bytes_digest(self.binary.read_bytes()),
                   'provider_config_sha256': digest(_read_json(self.provider_config))}
        if current != self.driver_identity:
            raise ValueError('Native binary or provider configuration changed')

    def _invoke(self, arguments, request, live):
        with self.cancellation_scope():
            return self._communicate(arguments, request, live)

    def _communicate(self, arguments, request, live):
        self.raise_if_cancelled()
        self._check_driver()
        environment = dict(os.environ)
        environment.pop('VEGA_HARNESS_API_KEY', None)
        if live:
            if not self._secret:
                raise RuntimeError('Native credential is not configured')
            environment['VEGA_HARNESS_API_KEY'] = self._secret
        self.raise_if_cancelled()
        process = subprocess.Popen([str(self.binary), *arguments, '--provider-config', str(self.provider_config)],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   env=environment, start_new_session=True)
        input_bytes = canonical_json(request).encode('utf-8')
        cancellation_sent = False
        while True:
            if self.cancel_requested and not cancellation_sent:
                try:
                    process.send_signal(self._cancel_signal)
                except ProcessLookupError:
                    pass
                cancellation_sent = True
            try:
                stdout, _ = process.communicate(input=input_bytes, timeout=0.1)
                break
            except subprocess.TimeoutExpired:
                input_bytes = None
            except KeyboardInterrupt:
                input_bytes = None
                self.request_cancel()
        if process.returncode != 0:
            self.raise_if_cancelled()
            raise RuntimeError('Native worker rejected preflight or execution')
        try:
            payload = json.loads(stdout)
        except (TypeError, ValueError) as error:
            raise RuntimeError('Native worker returned invalid protocol JSON') from error
        self._check_driver()
        return payload

    def prepare(self, request):
        result = self._invoke(['--preflight'], request, live=False)
        if (result.get('schema_version') != SCHEMA_VERSION
                or result.get('backend') != 'vega_native_runtime' or result.get('status') != 'ready'
                or result.get('transport_verified') is not False or result.get('live_calls') != 0
                or result.get('model') != request['model'] or result.get('limits') != request['limits']
                or result.get('hashes', {}).get('request') != digest(request)):
            raise ValueError('Native structural preflight does not match the requested trial')
        config_hash = result['hashes'].get('config')
        if not isinstance(config_hash, str) or not HASH_PATTERN.fullmatch(config_hash):
            raise ValueError('Native preflight omitted fixed configuration identity')
        if self.native_config_hash is not None and self.native_config_hash != config_hash:
            raise ValueError('Native fixed configuration changed between trials')
        self.native_config_hash = config_hash
        self.config_identity = config_hash
        self.preflight = result
        return result

    def capture_secret(self):
        if self._secret is None:
            self._secret = os.environ.get('VEGA_HARNESS_API_KEY')
        if not self._secret:
            raise ValueError('Set VEGA_HARNESS_API_KEY only in the explicit isolated research environment')

    def __call__(self, request, trial_root, run_id):
        self.capture_secret()
        return self._invoke(['--trial-root', str(Path(trial_root).absolute()), '--run-id', run_id],
                            request, live=True)


def _positive_budget(value):
    if value is None or not math.isfinite(value) or value <= 0:
        raise ValueError('A positive explicitly authorized study budget is required')
    return value


def _parser():
    parser = argparse.ArgumentParser(description='Native Vega harness pilot: plan before explicit research execution')
    parser.add_argument('command', choices=('plan', 'preflight', 'holdout'))
    parser.add_argument('--catalog', type=Path, default=DATA_ROOT / 'tasks.json')
    parser.add_argument('--baseline', type=Path, default=DATA_ROOT / 'baseline')
    parser.add_argument('--frozen', type=Path)
    parser.add_argument('--library', type=Path)
    parser.add_argument('--skill-approvals', type=Path)
    parser.add_argument('--native-binary', type=Path)
    parser.add_argument('--provider-config', type=Path)
    parser.add_argument('--limits', type=Path)
    parser.add_argument('--fixed-policy', type=Path)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--plan', type=Path)
    parser.add_argument('--runs-root', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--budget-usd', type=float)
    parser.add_argument('--isolation-label')
    return parser


def main(argv=None):
    args = _parser().parse_args(argv)
    try:
        catalog = load_catalog(args.catalog)
        missing = [name for name in ('frozen', 'library', 'skill_approvals', 'native_binary', 'provider_config')
                   if getattr(args, name) is None]
        if missing:
            document = {'schema_version': SCHEMA_VERSION, 'status': 'planned', 'execution_status': 'not_run',
                        'planned_trials': sum(task['split'] == 'holdout' for task in catalog['tasks'])
                        * args.repetitions * 2, 'model': DEFAULT_MODEL, 'missing_prerequisites': missing,
                        'cost_usd': None, 'live_calls': 0}
            if args.command != 'plan':
                raise ValueError('Missing explicit research prerequisites: ' + ', '.join(missing))
        else:
            limits = _read_json(args.limits) if args.limits else dict(DEFAULT_LIMITS)
            policy = args.fixed_policy.read_text(encoding='utf-8') if args.fixed_policy else FIXED_POLICY
            trusted = _read_json(args.skill_approvals)
            launcher = NativeProcessLauncher(args.native_binary, args.provider_config)
            request = make_request(catalog['tasks'][0], args.baseline, DEFAULT_MODEL, limits, policy, trusted)
            launcher.prepare(request)
            launcher.raise_if_cancelled()
            settings = dict(catalog_path=args.catalog, baseline_dir=args.baseline, frozen_dir=args.frozen,
                            library_dir=args.library, model=DEFAULT_MODEL, limits=limits, fixed_policy=policy,
                            trusted_skill_approvals=trusted, provider_identity=launcher_identity(launcher))
            if args.command == 'preflight':
                document = dict(launcher.preflight, driver_identity=launcher.driver_identity)
            elif args.command == 'plan':
                document = freeze_plan(**settings, repetitions=args.repetitions)
            elif not args.execute:
                document = {'status': 'not_run', 'live_calls': 0,
                            'reason': 'Explicit --execute, budget and operator isolation configuration are required'}
            else:
                _positive_budget(args.budget_usd)
                if not args.isolation_label or args.plan is None or args.runs_root is None:
                    raise ValueError('Provide the frozen --plan, --runs-root and operator --isolation-label')
                if launcher.preflight.get('pricing_configured') is not True:
                    raise ValueError('Configure explicit native pricing before a paid study')
                document = run_holdout(**settings, plan=_read_json(args.plan), runs_root=args.runs_root,
                                       launcher=launcher, budget_usd=args.budget_usd)
                document['operator_isolation_label'] = args.isolation_label
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            _write_json(args.output, document)
        else:
            print(canonical_json(document))
        return 0
    except StudyCancelled:
        print(canonical_json({'status': 'cancelled', 'native_result_obtained': False}))
        return 130
    except (ValueError, RuntimeError, OSError) as error:
        print('error: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
