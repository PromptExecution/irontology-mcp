#!/usr/bin/env python3

import base64
import json
import sys


def classify(path_hint: str, media_type: str, text: str) -> tuple[str | None, str | None]:
    lowered = text.lower()
    hint = (path_hint or "").lower()

    if "meeting-notes" in hint or "steering" in hint:
        return "doc:MeetingNotes", "shape:MeetingNotes"
    if media_type.endswith("presentationml.presentation") or hint.endswith(".pptx"):
        return "doc:Presentation", "shape:Presentation"
    if media_type == "application/pdf" or hint.endswith(".pdf"):
        return "doc:PolicyDocument", "shape:PolicyDocument"
    if "drawio" in hint or "process" in hint:
        return "doc:ProcessDiagram", "shape:ProcessDiagram"
    if "standing data" in lowered:
        return "doc:ArchitectureDocument", "shape:ArchitectureDocument"
    return None, None


def main() -> int:
    request = json.loads(sys.stdin.read().strip())
    args = request.get("params", {}).get("arguments", {})
    payload = args.get("payload", {})
    path_hint = payload.get("path_hint", "")
    media_type = payload.get("media_type", "")
    raw = base64.b64decode(payload.get("bytes_b64", ""))
    text = raw.decode("utf-8", errors="ignore")

    doc_class, shape = classify(path_hint, media_type, text)
    lowered = text.lower()

    fields = {}
    if "standing data" in lowered:
        fields["topic"] = "standing data"
    if "payment retries" in lowered:
        fields["topic_secondary"] = "payment retries"

    claims = []
    notes = []
    if "standing data" in lowered or "payment retries" in lowered:
        claims.append(
            {
                "id": "claim:python-curator:latent",
                "subject": "concept:acme:latent-knowledge",
                "predicate": "semantic:may_depend_on",
                "object": "view:acme:cross-document-correlation",
                "evidence": ["obs:python-curator"],
                "confidence": 0.67,
                "namespace": "ctx:acme",
            }
        )
        notes.append("python curator observed a cross-document dependency hint")

    response = {
        "jsonrpc": "2.0",
        "id": request.get("id", "forward-1"),
        "result": {
            "target": args.get("target", "stdio://child:python3 executors/python_curator.py"),
            "output": {
                "text": text,
                "fields": fields,
                "class": doc_class,
                "shape": shape,
                "claims": claims,
                "relations": [],
                "notes": notes,
                "has_symbols": False,
            },
            "trace": ["python-curator"],
            "artifacts": [],
        },
    }
    sys.stdout.write(json.dumps(response))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
