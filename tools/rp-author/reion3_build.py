"""Reproducible reion3 → RP project generator.

Reads reion3 `docs/plans/*.md` / `docs/reports/*.md`, derives node specs
(kind, title, date, supersedes / builds-on edges from cross-references and
filename dates), and emits a complete RP project tree suitable for
`rp validate` / `rp-view`. Idempotent: same inputs → same project bytes.

This tool only *describes* documentary provenance. It does not run
simulations, does not certify scientific validity, and does not modify
reion3 sources.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

KIND_BY_DIR = {"plans": "Method", "reports": "Interpretation"}
RECORD_DIR_BY_KIND = {
    "Method": "records/methods",
    "Interpretation": "records/interpretations",
    "Observation": "records/observations",
    "Question": "records/questions",
    "Hypothesis": "records/records/hypotheses",
}
PREFIX_BY_KIND = {
    "Method": "mth",
    "Interpretation": "int",
    "Observation": "obs",
    "Question": "qst",
    "Hypothesis": "hyp",
}
COUNTER_BASE = {
    "project": 1,
    "thread": 2_001,
    "question": 2_077,
    "method": 2_008,
    "interpretation": 2_060,
    "observation": 2_002,
    "artifact": 2_200,
    "reference": 2_300,
    "relation": 2_001,
}
PROJECT_ID = "proj_01KRE103C0VERAGE0000000001"
THREAD_ID = "thd_01KRE103C0VERAGE0000002001"
CREATED = "2026-09-14T00:00:00Z"
CREATED_BY = {"type": "agent", "id": "rp-author"}
ACCESS = {"level": "restricted", "compartments": []}
DATE_RE = re.compile(r"^(\d{4})-(\d{2})-(\d{2})-(.+)\.md$")
LINK_RE = re.compile(r"\]\((\d{4}-\d{2}-\d{2}-[^)]+\.md)\)")


def object_id(prefix: str, n: int) -> str:
    # rp/core ID pattern: <prefix>_ + 26 Crockford Base32 chars. The alpha-era
    # '01KRE103C0VERAGE' stamp is 16 chars, leaving a 10-char zero-padded counter.
    return f"{prefix}_01KRE103C0VERAGE{n:010d}"


def slug(text: str) -> str:
    return re.sub(r"-+", "-", re.sub(r"[^a-z0-9]+", "-", text.lower())).strip("-")


@dataclass
class Doc:
    path: Path
    date: str
    name: str
    kind: str
    title: str
    body: str
    links: list[str]


@dataclass
class Spec:
    doc: Doc
    node_id: str
    logical_id: str
    digest: str
    size: int


def parse_doc(path: Path, base: Path) -> Doc | None:
    m = DATE_RE.match(path.name)
    if not m:
        return None
    date = f"{m.group(1)}-{m.group(2)}-{m.group(3)}"
    text = path.read_text(encoding="utf-8")
    title = next((ln.strip("# ").strip() for ln in text.splitlines() if ln.startswith("#")), path.name)
    kind = KIND_BY_DIR.get(path.parent.name, "Observation")
    links = [lm for lm in LINK_RE.findall(text) if (path.parent / lm).exists()]
    return Doc(path.relative_to(base), date, m.group(4), kind, title, text, links)


def load_docs(reion3: Path) -> list[Doc]:
    docs = []
    for sub in ("plans", "reports"):
        d = reion3 / "docs" / sub
        if d.is_dir():
            docs.extend(filter(None, (parse_doc(p, reion3) for p in sorted(d.rglob("*.md")))))
    return docs


def yaml_str(v: str) -> str:
    return json.dumps(v, ensure_ascii=False)


def write_yaml(path: Path, obj: dict) -> None:
    lines = []

    def emit(key: str, value, indent: int) -> None:
        pad = "  " * indent
        if isinstance(value, dict):
            lines.append(f"{pad}{key}:")
            for k, v in value.items():
                emit(k, v, indent + 1)
        elif isinstance(value, list):
            if not value:
                lines.append(f"{pad}{key}: []")
            else:
                lines.append(f"{pad}{key}:")
                for item in value:
                    if isinstance(item, dict):
                        first = True
                        for k, v in item.items():
                            if first:
                                lines.append(f"{pad}  - {k}: {scalar(v)}")
                                first = False
                            else:
                                lines.append(f"{pad}    {k}: {scalar(v)}")
                    else:
                        lines.append(f"{pad}  - {scalar(item)}")
        else:
            lines.append(f"{pad}{key}: {scalar(value)}")

    def scalar(v):
        if v is None:
            return "null"
        if isinstance(v, bool):
            return "true" if v else "false"
        if isinstance(v, (int, float)):
            return str(v)
        return yaml_str(str(v))

    for k, v in obj.items():
        emit(k, v, 0)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _method_payload(doc, ref_id: str) -> dict:
    return {"method": {
        "version": doc.date,
        "procedure": doc.title,
        "applicable_inputs": [f"Derived from {doc.path.as_posix()}."],
        "protocol_references": [ref_id],
    }}


KIND_PAYLOAD_STATIC = {
    "Interpretation": lambda doc: {"interpretation": {
        "alternatives_considered": [f"See cross-referenced documents in {doc.path.as_posix()}."],
        "unresolved_alternatives": [],
    }},
    "Observation": lambda doc: {"observation": {
        "context": f"Documentary record from {doc.path.as_posix()}.",
        "method": {"description": "Document review; no new measurement."},
        "reproducibility_criteria": ["Re-read the cited source document."],
    }},
    "Question": lambda doc: {"question": {
        "resolution_criteria": ["Researcher review of the cited document."],
        "evidence_requirements": [f"{doc.path.as_posix()}"],
    }},
    "Hypothesis": lambda doc: {"hypothesis": {
        "falsification_criteria": ["Contradicted by a later dated report."],
        "revision_criteria": ["Superseded by a later dated plan or report."],
    }},
}


def kind_payload(kind: str, doc, art_id: str, ref_id: str) -> dict:
    if kind == "Method":
        return _method_payload(doc, ref_id)
    return KIND_PAYLOAD_STATIC.get(kind, KIND_PAYLOAD_STATIC["Observation"])(doc)



def node_yaml(spec: Spec, all_specs: dict[str, Spec], art_id: str, ref_id: str) -> dict:
    doc = spec.doc
    parents = []
    refs = []
    for lm in doc.links:
        target = next((s for s in all_specs.values() if s.doc.path.name == lm), None)
        if target and target.doc.date < doc.date:
            parents.append(target.node_id)
        elif target:
            refs.append(target.node_id)
    conditions = [f"{doc.path.as_posix()} (exact-line capture pending)"]
    payload = kind_payload(doc.kind, doc, art_id, ref_id)
    return {
        "schema": "rp/node-revision/v1",
        "id": spec.node_id,
        "logical_id": spec.logical_id,
        "kind": doc.kind,
        "record_state": "frozen",
        "title": f"{doc.date} {doc.name}",
        "statement": doc.title,
        "scope": {"statement": "Documentary provenance node; not a new measurement.", "conditions": conditions},
        "assumptions": [],
        "limitations": ["Agent-derived provenance; scientific validity requires researcher review."],
        "revision": {"parents": [], "summary": f"Derived from {doc.path.as_posix()}."},
        "created_at": CREATED,
        "created_by": CREATED_BY,
        "source": {"artifacts": [f"art_{spec.node_id[4:]}"]},
        "access": ACCESS,
        "tags": ["agent-candidate", "reion3", doc.path.parent.name],
        **payload,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--reion3", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    reion3 = args.reion3.resolve()
    docs = load_docs(reion3)
    if not docs:
        print("no dated docs found", file=sys.stderr)
        return 1

    counters = dict(COUNTER_BASE)
    specs: dict[str, Spec] = {}
    for doc in docs:
        key = f"{doc.kind}:{doc.path}"
        counter_key = doc.kind.lower()
        n = counters[counter_key]
        counters[counter_key] += 1
        node_id = object_id(PREFIX_BY_KIND[doc.kind], n)
        raw = doc.body.encode("utf-8")
        specs[key] = Spec(doc, node_id, f"reion3-{slug(doc.name)}", hashlib.sha256(raw).hexdigest(), len(raw))

    if args.dry_run:
        for s in sorted(specs.values(), key=lambda s: s.doc.path.as_posix()):
            print(f"{s.node_id}  {s.doc.kind:<14} {s.doc.path}")
        print(f"\n{len(specs)} nodes", file=sys.stderr)
        return 0

    root = args.out
    res = root / ".research"
    # Copy exact source captures into the project so artifact URIs resolve.
    src_root = root / "sources"
    for s in specs.values():
        dest = src_root / s.doc.path
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(s.doc.body, encoding="utf-8")
    for d in ("records/questions", "records/methods", "records/interpretations",
              "records/observations", "relations", "thread-bindings", "artifacts",
              "references", "threads"):
        (res / d).mkdir(parents=True, exist_ok=True)

    write_yaml(res / "project.yaml", {
        "schema": "rp/project/v1", "id": PROJECT_ID, "slug": "reion3-provenance",
        "title": "reion3 — full documentary provenance graph",
        "created_at": CREATED,
        "schema_policy": {"core_version": "v1", "kind_registry": "rp/kinds/v1", "allowed_extensions": []},
        "repository_policy": {"visibility": "private", "allowed_remotes": []},
        "access_defaults": ACCESS,
    })
    root_qid = object_id("qst", COUNTER_BASE["question"])
    write_yaml(res / "records/questions/root-question--{root_qid}.yaml".format(root_qid=root_qid), {
        "schema": "rp/node-revision/v1",
        "id": root_qid,
        "logical_id": "reion3-root-question",
        "kind": "Question",
        "record_state": "frozen",
        "title": "reion3 root provenance question",
        "statement": "What is the documented provenance of every plan and report in the reion3 project?",
        "scope": {"statement": "Whole-project documentary provenance.", "conditions": []},
        "assumptions": [],
        "limitations": ["Agent-derived provenance; scientific validity requires researcher review."],
        "revision": {"parents": [], "summary": "Synthetic root question for the whole-project thread."},
        "created_at": CREATED,
        "created_by": CREATED_BY,
        "source": {"artifacts": []},
        "access": ACCESS,
        "tags": ["agent-candidate", "reion3", "root"],
        "question": {
            "resolution_criteria": ["Every dated plan/report has a provenance node and derived-from links."],
            "evidence_requirements": ["docs/plans/*.md", "docs/reports/*.md"],
        },
    })
    write_yaml(res / "threads" / f"reion3--{THREAD_ID}.yaml", {
        "schema": "rp/research-thread/v1", "id": THREAD_ID,
        "title": "Whole-project reion3 research context",
        "objective": "Full documentary provenance across all dated plans and reports; agent-derived, researcher-reviewed.",
        "root_question_revision": root_qid,
        "created_at": CREATED, "created_by": CREATED_BY, "access": ACCESS,
    })

    artifact_ids: dict[str, str] = {}
    reference_ids: dict[str, str] = {}
    for s in specs.values():
        art_id = object_id("art", counters["artifact"])
        counters["artifact"] += 1
        artifact_ids[s.node_id] = art_id
        raw = s.doc.body.encode("utf-8")
        write_yaml(res / "artifacts" / f"{s.doc.name}--{art_id}.yaml", {
            "schema": "rp/artifact-manifest/v1", "id": art_id,
            "title": f"{s.doc.name} exact source",
            "uri": f"file:sources/{s.doc.path.as_posix()}",
            "media_type": "text/markdown", "size_bytes": s.size,
            "sha256": f"sha256:{s.digest}",
            "created_at": CREATED, "access": ACCESS,
        })
        ref_id = object_id("ref", counters["reference"])
        counters["reference"] += 1
        reference_ids[s.node_id] = ref_id
        write_yaml(res / "references" / f"{s.doc.name}--{ref_id}.yaml", {
            "schema": "rp/external-reference/v1", "id": ref_id, "type": "file",
            "canonical_uri": f"file:sources/{s.doc.path.as_posix()}",
            "foreign_key": None, "display_label": f"{s.doc.name} documentary source",
            "retrieved_at": CREATED,
            "source_version": {"identifier": f"reion3 docs capture", "digest": f"sha256:{s.digest}"},
            "trust_tags": ["local-documentary", "agent-selected", "not-new-science"],
            "access": ACCESS,
        })

    by_name = {s.doc.path.name: s for s in specs.values()}
    rel_n = counters["relation"]
    for s in specs.values():
        node = node_yaml(s, specs, artifact_ids[s.node_id], reference_ids[s.node_id])
        node["source"]["artifacts"] = [artifact_ids[s.node_id]]
        record_dir = RECORD_DIR_BY_KIND.get(s.doc.kind, "records/observations")
        write_yaml(res / record_dir / f"{s.doc.name}--{s.node_id}.yaml", node)
        for lm in s.doc.links:
            target = by_name.get(lm)
            if not target:
                continue
            rel_id = object_id("rel", rel_n)
            rel_n += 1
            typ = "derived-from"
            write_yaml(res / "relations" / f"{slug(s.doc.name)}-{slug(target.doc.name)}--{rel_id}.yaml", {
                "schema": "rp/scientific-relation-revision/v1", "id": rel_id,
                "logical_id": f"reion3-edge-{rel_n:03d}",
                "record_state": "frozen", "type": typ,
                "from_revision": s.node_id, "to_revision": target.node_id,
                "scope": {"statement": "Documentary cross-reference only.", "conditions": [], "exclusions": []},
                "assertion_mode": "inferred", "relation_state": "active",
                "rationale": f"Cross-reference in {s.doc.path.as_posix()}.",
                "revision": {"parents": [], "summary": "Auto-derived from markdown link."},
                "created_at": CREATED, "created_by": CREATED_BY,
                "source": {"artifacts": [artifact_ids[s.node_id]]},
                "access": ACCESS,
            })

    print(f"wrote {len(specs)} nodes to {root}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
