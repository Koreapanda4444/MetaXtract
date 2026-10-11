from __future__ import annotations

import base64
import binascii
import hashlib
import hmac
from pathlib import Path
from typing import Any

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)

from ..core.files import PathLike


SIGNATURE_ALGORITHM = "Ed25519"
SIGNATURE_MEMBER = "signature.json"


class SignatureValidationError(ValueError):
    def __init__(self, issue: str, detail: str | None = None):
        self.issue = issue
        self.detail = detail
        super().__init__(detail or issue)


def _public_key_bytes(public_key: Ed25519PublicKey) -> bytes:
    return public_key.public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )


def _public_key_fingerprint(public_key: Ed25519PublicKey) -> str:
    return hashlib.sha256(_public_key_bytes(public_key)).hexdigest()


def _write_key(path: Path, data: bytes, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with path.open("xb") as stream:
            stream.write(data)
        path.chmod(mode)
    except Exception:
        path.unlink(missing_ok=True)
        raise


def generate_signing_keypair(
    private_key_path: PathLike,
    public_key_path: PathLike,
) -> None:
    private_path = Path(private_key_path)
    public_path = Path(public_key_path)
    if private_path.resolve(strict=False) == public_path.resolve(strict=False):
        raise ValueError("private and public key paths must be different")
    for path in (private_path, public_path):
        if path.exists():
            raise FileExistsError(f"refusing to overwrite key: {path}")

    private_key = Ed25519PrivateKey.generate()
    private_data = private_key.private_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PrivateFormat.PKCS8,
        encryption_algorithm=serialization.NoEncryption(),
    )
    public_data = private_key.public_key().public_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PublicFormat.SubjectPublicKeyInfo,
    )

    _write_key(public_path, public_data, 0o644)
    try:
        _write_key(private_path, private_data, 0o600)
    except Exception:
        public_path.unlink(missing_ok=True)
        raise


def _load_private_key(path: PathLike) -> Ed25519PrivateKey:
    try:
        key = serialization.load_pem_private_key(Path(path).read_bytes(), password=None)
    except (OSError, TypeError, ValueError) as exc:
        raise ValueError(f"invalid Ed25519 private key: {path}") from exc
    if not isinstance(key, Ed25519PrivateKey):
        raise ValueError(f"key is not an Ed25519 private key: {path}")
    return key


def _load_public_key(path: PathLike) -> Ed25519PublicKey:
    try:
        key = serialization.load_pem_public_key(Path(path).read_bytes())
    except (OSError, TypeError, ValueError) as exc:
        raise ValueError(f"invalid Ed25519 public key: {path}") from exc
    if not isinstance(key, Ed25519PublicKey):
        raise ValueError(f"key is not an Ed25519 public key: {path}")
    return key


def sign_manifest(manifest_data: bytes, private_key_path: PathLike) -> dict[str, str]:
    private_key = _load_private_key(private_key_path)
    public_key = private_key.public_key()
    return {
        "algorithm": SIGNATURE_ALGORITHM,
        "manifest_sha256": hashlib.sha256(manifest_data).hexdigest(),
        "public_key_sha256": _public_key_fingerprint(public_key),
        "signature": base64.b64encode(private_key.sign(manifest_data)).decode("ascii"),
    }


def verify_manifest_signature(
    manifest_data: bytes,
    signature_record: Any,
    public_key_path: PathLike,
) -> None:
    if not isinstance(signature_record, dict):
        raise SignatureValidationError("invalid_signature_record")
    if signature_record.get("algorithm") != SIGNATURE_ALGORITHM:
        raise SignatureValidationError("unsupported_signature_algorithm")

    expected_manifest_hash = signature_record.get("manifest_sha256")
    actual_manifest_hash = hashlib.sha256(manifest_data).hexdigest()
    if not isinstance(expected_manifest_hash, str) or not hmac.compare_digest(
        expected_manifest_hash,
        actual_manifest_hash,
    ):
        raise SignatureValidationError("manifest_signature_hash_mismatch")

    try:
        public_key = _load_public_key(public_key_path)
    except ValueError as exc:
        raise SignatureValidationError("invalid_public_key", str(exc)) from exc

    expected_fingerprint = signature_record.get("public_key_sha256")
    actual_fingerprint = _public_key_fingerprint(public_key)
    if not isinstance(expected_fingerprint, str) or not hmac.compare_digest(
        expected_fingerprint,
        actual_fingerprint,
    ):
        raise SignatureValidationError("public_key_mismatch")

    encoded_signature = signature_record.get("signature")
    if not isinstance(encoded_signature, str):
        raise SignatureValidationError("invalid_signature_encoding")
    try:
        signature = base64.b64decode(encoded_signature, validate=True)
    except (binascii.Error, ValueError) as exc:
        raise SignatureValidationError("invalid_signature_encoding") from exc
    if len(signature) != 64:
        raise SignatureValidationError("invalid_signature_encoding")

    try:
        public_key.verify(signature, manifest_data)
    except InvalidSignature as exc:
        raise SignatureValidationError("signature_verification_failed") from exc
