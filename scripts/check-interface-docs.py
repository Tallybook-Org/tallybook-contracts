#!/usr/bin/env python3
"""Assert the README describes the interface the contracts actually compile to.

The source of truth is the built wasm, read back with
`stellar contract info interface`, which reports the exported functions, the
error enum with its discriminants, and every event's topics. Each is compared
against what the README's section for that contract documents.

This exists because the README is the interface for everyone who is not
reading the Rust: an indexer matching on error discriminants, a collector
building calls, a buyer checking a charge. Drift there is not cosmetic. Two
error doc comments in statement-registry had already fallen behind the code
before this check was written.

Usage:
    scripts/check-interface-docs.py                 # after `make build`
    scripts/check-interface-docs.py --selftest      # prove the check still fires
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Contract name as it appears in the README's `### `name`` heading, mapped to
# the wasm `make build` produces for it.
CONTRACTS = {
    "price_book": "target/wasm32v1-none/release/price_book.wasm",
    "statement_registry": "target/wasm32v1-none/release/statement_registry.wasm",
}


def contract_interface(wasm: Path) -> str:
    result = subprocess.run(
        ["stellar", "contract", "info", "interface", "--wasm", str(wasm)],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        sys.exit(f"stellar contract info interface failed for {wasm}:\n{result.stderr}")
    return result.stdout


def parse_wasm(interface: str) -> tuple[set[str], dict[str, int], set[tuple[str, ...]]]:
    functions = set(re.findall(r"^\s{4}fn ([a-z_0-9]+)\(", interface, re.MULTILINE))

    errors: dict[str, int] = {}
    enum_body = re.search(r"pub enum Error \{(.*?)\n\}", interface, re.DOTALL)
    if enum_body:
        for name, value in re.findall(r"([A-Za-z0-9]+) = (\d+),", enum_body.group(1)):
            errors[name] = int(value)

    events = set()
    for topics in re.findall(r"contractevent\(topics = \[([^\]]+)\]\)", interface):
        events.add(tuple(re.findall(r'"([^"]+)"', topics)))

    return functions, errors, events


def readme_section(readme: str, contract: str) -> str:
    """The README text documenting one contract, from its heading to the next."""
    heading = readme.index(f"### `{contract}`")
    # Start after the heading line itself, so the heading does not match as
    # its own terminator.
    body = readme[readme.index("\n", heading) :]
    end = re.search(r"^## |^### `", body, re.MULTILINE)
    return body[: end.start()] if end else body


def parse_readme(section: str) -> tuple[set[str], dict[str, int], set[tuple[str, ...]]]:
    functions = set(re.findall(r"^- \*\*`([a-z_0-9]+)\(", section, re.MULTILINE))

    errors: dict[str, int] = {}
    for value, name in re.findall(r"^\| (\d+) \| `([A-Za-z0-9]+)`", section, re.MULTILINE):
        errors[name] = int(value)

    events = set()
    for a, b in re.findall(r'topics `\("([^"]+)", "([^"]+)"\)`', section):
        events.add((a, b))

    return functions, errors, events


def compare(contract: str, wasm_side, readme_side) -> list[str]:
    wasm_fns, wasm_errs, wasm_events = wasm_side
    doc_fns, doc_errs, doc_events = readme_side
    problems = []

    for missing in sorted(wasm_fns - doc_fns):
        problems.append(f"{contract}: `{missing}` is exported but not documented in the README")
    for extra in sorted(doc_fns - wasm_fns):
        problems.append(f"{contract}: README documents `{extra}`, which the wasm does not export")

    for name, value in sorted(wasm_errs.items()):
        if name not in doc_errs:
            problems.append(f"{contract}: error `{name}` = {value} is not in the README's table")
        elif doc_errs[name] != value:
            problems.append(
                f"{contract}: error `{name}` is {value} in the wasm "
                f"but {doc_errs[name]} in the README"
            )
    for name in sorted(set(doc_errs) - set(wasm_errs)):
        problems.append(f"{contract}: README documents error `{name}`, which the wasm does not define")

    for missing in sorted(wasm_events - doc_events):
        problems.append(f"{contract}: event topics {missing} are emitted but not documented")
    for extra in sorted(doc_events - wasm_events):
        problems.append(f"{contract}: README documents event topics {extra}, which nothing emits")

    return problems


# Each case mutates the real README in memory and names the substring the
# resulting complaint must contain. A check that cannot fail is not a check,
# and these run in CI ahead of the real comparison for that reason -- the same
# reason scripts/audit-pr-malware.py proves its own rules still fire.
SELFTEST_CASES = [
    (
        "a documented function removed",
        lambda r: r.replace("- **`version_at(operator: Address, ledger: u32)", "- **`removed_name(x: u32)"),
        "is exported but not documented",
    ),
    (
        "an error discriminant renumbered",
        lambda r: r.replace("| 13 | `ProofTooLong`", "| 31 | `ProofTooLong`"),
        "in the wasm but 31 in the README",
    ),
    (
        "an event topic renamed",
        lambda r: r.replace('topics `("statement", "anchor")`', 'topics `("statement", "anchored")`'),
        "are emitted but not documented",
    ),
]


def selftest(readme: str) -> int:
    wasm_sides = {}
    for contract, wasm_rel in CONTRACTS.items():
        wasm = REPO_ROOT / wasm_rel
        if not wasm.exists():
            sys.exit(f"{wasm} not found — run `make build` first")
        wasm_sides[contract] = parse_wasm(contract_interface(wasm))

    failures = 0
    for label, mutate, expected in SELFTEST_CASES:
        mutated = mutate(readme)
        if mutated == readme:
            print(f"  BROKEN  {label}: the mutation did not change the README", file=sys.stderr)
            failures += 1
            continue

        problems = []
        for contract, wasm_side in wasm_sides.items():
            readme_side = parse_readme(readme_section(mutated, contract))
            problems.extend(compare(contract, wasm_side, readme_side))

        if any(expected in problem for problem in problems):
            print(f"  caught  {label}")
        else:
            print(f"  MISSED  {label}: expected a complaint containing {expected!r}", file=sys.stderr)
            for problem in problems:
                print(f"            got: {problem}", file=sys.stderr)
            failures += 1

    if failures:
        print(f"\n{failures} selftest case(s) not caught", file=sys.stderr)
        return 1
    print("selftest: every mutation was caught")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--readme", default=str(REPO_ROOT / "README.md"))
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="prove the comparison still catches known drift, then exit",
    )
    args = parser.parse_args()

    readme = Path(args.readme).read_text()

    if args.selftest:
        return selftest(readme)

    problems: list[str] = []
    for contract, wasm_rel in CONTRACTS.items():
        wasm = REPO_ROOT / wasm_rel
        if not wasm.exists():
            sys.exit(f"{wasm} not found — run `make build` first")
        wasm_side = parse_wasm(contract_interface(wasm))
        readme_side = parse_readme(readme_section(readme, contract))

        # A parse that finds nothing is a broken parser, not a clean repo.
        if not wasm_side[0] or not readme_side[0]:
            sys.exit(f"{contract}: parsed no functions from one side — the check itself is broken")

        problems.extend(compare(contract, wasm_side, readme_side))

    if problems:
        print("README does not match the compiled contracts:\n", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        print(
            "\nUpdate README.md to match, or revert the interface change.",
            file=sys.stderr,
        )
        return 1

    print("README matches both contracts' exported functions, errors and events.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
