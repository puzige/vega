import argparse
from collections import Counter
from dataclasses import dataclass
import json
from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
HAN = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\U00020000-\U000323af]")
PLACEHOLDER = re.compile(r"\{([A-Za-z_][A-Za-z_0-9]*)\}")
RAW_STRING = re.compile(r'(?:br|cr|r)(#*)"')
CHAR_LITERAL = re.compile(r"(?:b)?'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'")
IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z_0-9]*")
CATEGORIES = {
    "log",
    "benchmark",
    "fixture",
    "model_prompt",
    "internal",
    "protocol",
    "stored_content",
    "config_template",
    "ui",
    "unreviewed",
}
SPEC_FIXTURES = [
    "crates/vega_ui/src/conversation_stream/bench.rs",
    "crates/vega_ui/src/conversation_stream/render_rows.rs",
]


@dataclass
class Token:
    kind: str
    text: str
    value: str
    line: int
    offset: int


def decode_rust(text):
    def escape(match):
        value = match.group(1)
        if value.startswith("u{"):
            return chr(int(value[2:-1].replace("_", ""), 16))
        if value.startswith("x"):
            return chr(int(value[1:], 16))
        if value[0] in "\r\n":
            return ""
        return {"n": "\n", "r": "\r", "t": "\t", "0": "\0"}.get(value, value)

    return re.sub(r"\\(u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|\r?\n\s*|.)", escape, text)


def rust_tokens(source):
    tokens = []
    errors = []
    index = 0
    line = 1
    size = len(source)
    while index < size:
        start = index
        start_line = line
        char = source[index]
        if char.isspace():
            index += 1
            line += char == "\n"
            continue
        if source.startswith("//", index):
            end = source.find("\n", index)
            index = size if end < 0 else end
            continue
        if source.startswith("/*", index):
            depth = 1
            index += 2
            while index < size and depth:
                if source.startswith("/*", index):
                    depth += 1
                    index += 2
                elif source.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    line += source[index] == "\n"
                    index += 1
            if depth:
                errors.append(f"line {start_line}: unterminated block comment")
            continue
        raw = RAW_STRING.match(source, index) if char in "bcr" else None
        if raw:
            body = raw.end()
            closing = '"' + raw.group(1)
            end = source.find(closing, body)
            if end < 0:
                errors.append(f"line {start_line}: unterminated raw string")
                end = size
                index = size
            else:
                index = end + len(closing)
            tokens.append(Token("string", source[start:index], source[body:end], start_line, start))
            line += source[start:index].count("\n")
            continue
        prefix = 1 if char in "bc" and source[index + 1:index + 2] == '"' else 0
        if char == '"' or prefix:
            index += prefix + 1
            body = index
            while index < size:
                if source[index] == "\\":
                    index += 2
                elif source[index] == '"':
                    break
                else:
                    index += 1
            if index >= size:
                errors.append(f"line {start_line}: unterminated string")
            value = decode_rust(source[body:index])
            index = min(index + 1, size)
            tokens.append(Token("string", source[start:index], value, start_line, start))
            line += source[start:index].count("\n")
            continue
        char_literal = CHAR_LITERAL.match(source, index) if char in "b'" else None
        if char_literal:
            index = char_literal.end()
            tokens.append(Token("char", source[start:index], source[start:index], start_line, start))
            continue
        identifier = IDENTIFIER.match(source, index) if char.isascii() and (char.isalpha() or char == "_") else None
        if identifier:
            index = identifier.end()
            tokens.append(Token("identifier", source[start:index], source[start:index], start_line, start))
            continue
        index += 1
        tokens.append(Token("punctuation", char, char, start_line, start))
    return tokens, errors


def placeholders(value, location, errors):
    names = set()
    index = 0
    while index < len(value):
        if value.startswith("{{", index) or value.startswith("}}", index):
            index += 2
        elif value[index] == "{":
            match = PLACEHOLDER.match(value, index)
            if match is None:
                errors.append(f"{location}: invalid or non-named placeholder")
                index += 1
            else:
                names.add(match.group(1))
                index = match.end()
        elif value[index] == "}":
            errors.append(f"{location}: unmatched closing brace")
            index += 1
        else:
            index += 1
    return names


def catalogs(root, errors):
    locales = {"en": {}, "zh-CN": {}}
    files = {"en": [], "zh-CN": []}
    directory = root / "crates/vega_i18n/locales"
    for path in sorted(directory.glob("*.toml")):
        language = next((language for language in locales if path.name == f"{language}.toml" or path.name.endswith(f".{language}.toml")), None)
        if language is None:
            errors.append(f"{path.relative_to(root)}: unrecognized catalog language")
            continue
        relative = str(path.relative_to(root))
        files[language].append(relative)
        try:
            values = tomllib.loads(path.read_text())
        except (OSError, tomllib.TOMLDecodeError) as error:
            errors.append(f"{relative}: {error}")
            continue
        for key, value in values.items():
            if not isinstance(value, str):
                errors.append(f"{relative}: {key}: catalog entries must be flat strings")
                continue
            if key in locales[language]:
                errors.append(f"{relative}: duplicate {language} key {key}")
            locales[language][key] = value
    for language in locales:
        if not files[language]:
            errors.append(f"no {language} catalogs found")
    en, zh = locales["en"], locales["zh-CN"]
    for key in sorted(en.keys() - zh.keys()):
        errors.append(f"missing zh-CN key: {key}")
    for key in sorted(zh.keys() - en.keys()):
        errors.append(f"missing en key: {key}")
    for key in sorted(en.keys() & zh.keys()):
        english = placeholders(en[key], f"en:{key}", errors)
        chinese = placeholders(zh[key], f"zh-CN:{key}", errors)
        if english != chinese:
            errors.append(f"placeholder mismatch {key}: en={sorted(english)}, zh-CN={sorted(chinese)}")
    return locales, files


def translation_calls(tokens, relative, english, errors):
    calls = 0
    for index, token in enumerate(tokens):
        if token.kind != "identifier" or token.text not in {"t", "t_args"}:
            continue
        if index and tokens[index - 1].text in {"fn", "."}:
            continue
        if index + 2 >= len(tokens) or tokens[index + 1].text != "(" or tokens[index + 2].kind != "string":
            continue
        key = tokens[index + 2].value
        calls += 1
        location = f"{relative}:{token.line}"
        if key not in english:
            errors.append(f"{location}: missing literal translation key: {key}")
            continue
        names = placeholders(english[key], f"en:{key}", errors)
        if token.text == "t":
            if names:
                errors.append(f"{location}: t({key}) requires t_args for {sorted(names)}")
            continue
        cursor = index + 3
        depth = 1
        argument_tokens = []
        while cursor < len(tokens) and depth:
            current = tokens[cursor]
            if current.kind == "punctuation" and current.text == "(":
                depth += 1
            elif current.kind == "punctuation" and current.text == ")":
                depth -= 1
            if depth:
                argument_tokens.append(current)
            cursor += 1
        if len(argument_tokens) < 3 or [item.text for item in argument_tokens[:3]] != [",", "&", "["]:
            errors.append(f"{location}: t_args({key}) argument names require manual review (nonliteral array)")
            continue
        provided = []
        stack = []
        literal_names = True
        for offset, current in enumerate(argument_tokens[2:], 2):
            if current.kind != "punctuation":
                continue
            if current.text == "(" and stack == ["["]:
                next_tokens = argument_tokens[offset + 1:offset + 3]
                if len(next_tokens) != 2 or next_tokens[0].kind != "string" or next_tokens[1].text != ",":
                    literal_names = False
                else:
                    provided.append(next_tokens[0].value)
            if current.text in {"[", "(", "{"}:
                stack.append(current.text)
            elif current.text in {"]", ")", "}"}:
                if stack:
                    stack.pop()
                if not stack:
                    break
            elif stack == ["["] and current.text != ",":
                literal_names = False
        if not literal_names:
            errors.append(f"{location}: t_args({key}) has dynamic argument names; manual review required")
        if set(provided) != names or len(provided) != len(set(provided)):
            errors.append(f"{location}: argument mismatch {key}: expected={sorted(names)}, provided={provided}")
    return calls


def inventory(root, english, errors):
    path = root / "scripts/i18n_allowlist.json"
    data = json.loads(path.read_text()) if path.exists() else {"entries": []}
    entries = {}
    for entry in data.get("entries", []):
        identity = (entry["path"], entry["literal"])
        if identity in entries:
            errors.append(f"duplicate inventory classification: {identity}")
        if entry.get("category") not in CATEGORIES or not entry.get("reason", "").strip() or not isinstance(entry.get("count"), int) or entry["count"] < 1:
            errors.append(f"invalid inventory classification: {identity}")
        entries[identity] = entry
    findings = []
    counts = Counter()
    calls = 0
    source_files = sorted((root / "crates").rglob("*.rs"))
    for source in source_files:
        relative = str(source.relative_to(root))
        tokens, lexical_errors = rust_tokens(source.read_text())
        errors.extend(f"{relative}: {error}" for error in lexical_errors)
        calls += translation_calls(tokens, relative, english, errors)
        for token in tokens:
            if token.kind != "string" or not HAN.search(token.value):
                continue
            identity = (relative, token.value)
            counts[identity] += 1
            entry = entries.get(identity, {})
            findings.append({
                "path": relative,
                "line": token.line,
                "literal": token.value,
                "category": entry.get("category", "unreviewed"),
                "reason": entry.get("reason", "No per-string classification; human review required."),
            })
    for identity, entry in entries.items():
        if counts[identity] != entry["count"]:
            errors.append(f"stale classification {identity[0]}:{identity[1]!r}: expected {entry['count']} occurrences, found {counts[identity]}")
    return findings, len(source_files), calls


def main():
    parser = argparse.ArgumentParser(description="Static catalog/reference checks and per-string Chinese Rust inventory; does not execute Vega or establish A01-A07.")
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--json", action="store_true", help="Print the full report as JSON.")
    parser.add_argument("--catalog-only", action="store_true", help="Skip Rust inventory and translation references.")
    arguments = parser.parse_args()
    root = arguments.root.resolve()
    errors = []
    try:
        locales, files = catalogs(root, errors)
        findings, sources, calls = ([], 0, 0) if arguments.catalog_only else inventory(root, locales["en"], errors)
    except (OSError, ValueError, KeyError) as error:
        print(f"Static inventory failed: {error}", file=sys.stderr)
        return 2
    categories = Counter(finding["category"] for finding in findings)
    unreviewed = categories["unreviewed"] + categories["ui"]
    report = {
        "catalog_files": files,
        "catalog_keys": {language: len(values) for language, values in locales.items()},
        "rust_files": sources,
        "inventory_scanned": not arguments.catalog_only,
        "literal_translation_calls": calls,
        "spec_fixture_files": [{"path": path, "exists": (root / path).is_file(), "policy": "Preserve samples; classify individual literals, never suppress the entire file."} for path in SPEC_FIXTURES],
        "remaining_chinese_literals": findings,
        "classification_counts": dict(sorted(categories.items())),
        "errors": errors,
        "limitations": [
            "Static checks do not prove language switching, persistence, accessibility, or A01-A07.",
            "English UI coverage and dynamically constructed messages require human review.",
            "Only literal t/t_args references and literal argument-name arrays are checked.",
            "Comments and character literals are excluded by the Rust lexer; raw strings are included.",
            "Allowlist entries match exact file and decoded string with an exact occurrence count; no file-wide exclusions.",
        ],
    }
    if arguments.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print(f"Catalog keys: {report['catalog_keys']}; Rust files: {sources}; literal calls: {calls}")
        if arguments.catalog_only:
            print("Rust reference and Chinese literal inventory: NOT RUN (--catalog-only)")
        for fixture in report["spec_fixture_files"]:
            print(f"SPEC FIXTURE {fixture['path']} (exists={fixture['exists']}): {fixture['policy']}")
        for finding in findings:
            print(f"{finding['path']}:{finding['line']}: [{finding['category']}] {json.dumps(finding['literal'], ensure_ascii=False)} — {finding['reason']}")
        print(f"Remaining Chinese literals by classification: {dict(sorted(categories.items()))}")
        for error in errors:
            print(f"ERROR {error}")
        for limitation in report["limitations"]:
            print(f"LIMIT {limitation}")
        finding_status = "NOT SCANNED" if arguments.catalog_only else str(unreviewed)
        print(f"Static result: catalog/reference errors={len(errors)}; UI/unreviewed findings={finding_status}")
    return 1 if errors or unreviewed else 0


if __name__ == "__main__":
    sys.exit(main())
