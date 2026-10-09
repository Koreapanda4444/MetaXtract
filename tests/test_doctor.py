from metaxtract.doctor import check_python_deps, run_doctor


def test_dependency_checks_use_import_module_names():
    dependencies = {item["name"]: item for item in check_python_deps()}

    assert dependencies["cryptography"]["found"] is True
    assert dependencies["Pillow"]["module"] == "PIL"
    assert dependencies["python-docx"]["module"] == "docx"
    assert dependencies["pypdf"]["module"] == "pypdf"
    assert dependencies["Pillow"]["found"] is True
    assert dependencies["python-docx"]["found"] is True


def test_doctor_reports_required_and_optional_status():
    result = run_doctor()

    assert result["ok"] is True
    assert all(item["required"] for item in result["python_packages"])
    assert all(not item["required"] for item in result["binaries"])
