"""Notices and relinkable sources for the pinned, statically linked AppImage runtime."""
from __future__ import annotations

import hashlib
import io
import json
import tarfile
import urllib.request
from pathlib import Path

# Match type2-runtime 20251108's reviewed dependency build recipe. Only the
# LGPL libfuse library is linked; its GPL command-line tools are not distributed.
INPUTS = {
    "runtime.tar.gz": ("https://github.com/AppImage/type2-runtime/archive/dd6cebedcbddde9c82f89b011e8e1d40b6e43868.tar.gz", "f5fec23be76e50e2445ed2d018bac49b367490fc483c62c4637e99ec705d27ba"),
    "fuse.tar.xz": ("https://github.com/libfuse/libfuse/releases/download/fuse-3.15.0/fuse-3.15.0.tar.xz", "70589cfd5e1cff7ccd6ac91c86c01be340b227285c5e200baa284e401eea2ca0"),
    "squashfuse.tar.gz": ("https://github.com/vasi/squashfuse/archive/0.5.2.tar.gz", "db0238c5981dabbd80ee09ae15387f390091668ca060a7bc38047912491443d3"),
    "musl-COPYRIGHT": ("https://git.musl-libc.org/cgit/musl/plain/COPYRIGHT?h=v1.2.5", "f9bc4423732350eb0b3f7ed7e91d530298476f8fec0c6c427a1c04ade22655af"),
    "mimalloc-LICENSE": ("https://raw.githubusercontent.com/microsoft/mimalloc/v2.1.7/LICENSE", "19c99805e7a44a34b297a75d1edea9985e300066dfc024d5c99d4236d4573b5d"),
    "zstd-LICENSE": ("https://raw.githubusercontent.com/facebook/zstd/v1.5.6/LICENSE", "7055266497633c9025b777c78eb7235af13922117480ed5c674677adc381c9d8"),
    "zlib-LICENSE": ("https://raw.githubusercontent.com/madler/zlib/v1.3.1/LICENSE", "845efc77857d485d91fb3e0b884aaa929368c717ae8186b66fe1ed2495753243"),
}
RELINK = """AppImage runtime sources / AppImage 런타임 원본

runtime.tar.gz: unmodified type2-runtime commit dd6cebedcbddde9c82f89b011e8e1d40b6e43868
fuse.tar.xz: libfuse 3.15.0 (the LGPL-2.1 library, not GPL utilities)
squashfuse.tar.gz: squashfuse 0.5.2

Extract runtime.tar.gz and read BUILD.md, scripts/docker/Dockerfile,
scripts/common/install-dependencies.sh and src/runtime/Makefile.
The unmodified runtime tree includes its libfuse mount.c patch and complete
build scripts. Its Alpine 3.21 toolchain supplies musl, zstd, zlib and mimalloc;
their original notices accompany the runtime. The exact libfuse/squashfuse
source archives used by the upstream recipe are provided here as well.

To rebuild: run ARCH=x86_64 scripts/docker/build-with-docker.sh from the
extracted runtime tree, using Docker and network access for the Alpine build
packages. The output is out/runtime-x86_64. To modify/relink libfuse, unpack
fuse.tar.xz, change its library sources, and adapt the Dockerfile to COPY your
source tree instead of the install script's wget/extract step; keep the
upstream patch/build options as appropriate. The complete runtime source is
provided so no proprietary object files are required for relinking.
Then rebuild the MetaXtract AppDir with the replacement runtime via
appimagetool --runtime-file /absolute/path/to/runtime-x86_64 APPDIR OUTPUT.
No publisher signature or cryptographic lock prevents replacement.

원본 압축을 풀고 BUILD.md와 빌드 스크립트를 참고하세요. libfuse를 수정하려면
제공한 소스를 풀어 수정하고 Dockerfile에서 해당 소스를 COPY하도록 변경한 뒤
런타임을 다시 빌드합니다. appimagetool의 --runtime-file에 새 런타임을 지정하면
다시 묶을 수 있습니다. 재연결에 필요한 런타임·라이브러리 원본과 패치를 포함하며
교체를 막는 배포자 서명이나 암호화 잠금은 없습니다.
"""


def records() -> list[dict]:
    return [{"file": name, "url": url, "sha256": sha} for name, (url, sha) in INPUTS.items()]


def checked(path: Path, expected: str) -> bytes:
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("AppImage source/notice checksum differs: " + path.name)
    return data


def prepare(project: Path, root: Path) -> list[dict]:
    cache = project / "target/appimage-sources"
    cache.mkdir(parents=True, exist_ok=True)
    notices = root / "licenses/appimage"
    notices.mkdir(parents=True)
    for name, (url, sha) in INPUTS.items():
        path = cache / name
        if not path.exists():
            with urllib.request.urlopen(url, timeout=60) as response:
                data = response.read(20 * 1024 * 1024 + 1)
            if len(data) > 20 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != sha:
                raise ValueError("AppImage source/notice download differs: " + name)
            path.write_bytes(data)
        data = checked(path, sha)
        if ".tar." not in name:
            (notices / name).write_bytes(data)
        else:
            with tarfile.open(fileobj=io.BytesIO(data)) as archive:
                for entry in archive:
                    if entry.isfile() and entry.name.count("/") == 1 and Path(entry.name).name in ("LICENSE", "LGPL2.txt"):
                        with archive.extractfile(entry) as stream:
                            (notices / (name + "-" + Path(entry.name).name)).write_bytes(stream.read())
    (notices / "relinking.txt").write_text(RELINK, encoding="utf-8")
    (notices / "inputs.json").write_text(json.dumps(records(), indent=2) + "\n", encoding="utf-8")
    return records()


def add_sources(archive: tarfile.TarFile, project: Path) -> list[dict]:
    cache = project / "target/appimage-sources"
    if not cache.is_dir():
        return []
    prefix = "MetaXtract-sources/distribution/AppImage/"
    for name, (_, sha) in INPUTS.items():
        data = checked(cache / name, sha)
        entry = tarfile.TarInfo(prefix + name)
        entry.size = len(data)
        archive.addfile(entry, io.BytesIO(data))
    data = RELINK.encode("utf-8")
    entry = tarfile.TarInfo(prefix + "relinking.txt")
    entry.size = len(data)
    archive.addfile(entry, io.BytesIO(data))
    return records()
