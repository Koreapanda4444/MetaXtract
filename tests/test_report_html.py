from report_html import render_html


def sample_records():
    return [
        {
            "path": "a.jpg",
            "mime": "image/jpeg",
            "size_bytes": 1234,
            "metadata": {"author": "alice", "gps": [37.5, 127.0]},
            "warnings": ["low_resolution"],
            "errors": [],
        },
        {
            "path": "b.docx",
            "mime": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "size_bytes": 4321,
            "metadata": {"author": "bob", "producer": "Word"},
            "warnings": [],
            "errors": ["corrupt"],
        },
    ]


def test_render_html_basic():
    html = render_html(sample_records())
    assert "MetaXtract Report" in html
    assert "파일 개수" in html
    assert "image/jpeg" in html
    assert "application/vnd.openxmlformats-officedocument.wordprocessingml.document" in html
    assert "alice" in html
    assert "bob" in html
    assert "Warnings" in html
    assert "Errors" in html
    assert "low_resolution" in html
    assert "corrupt" in html
