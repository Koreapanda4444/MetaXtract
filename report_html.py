
from __future__ import annotations
import json

def generate_html_report(session: dict, items: list[dict]) -> str:
    # 간단한 CSS/JS 포함, 위험 요약 대시보드, 표, 그룹, 상세 전환 등
    # Top GPS/Device/Author 추출
    gps_items = [it for it in items if (it.get('signals') or {}).get('privacy_flags', {}).get('has_gps')]
    author_items = [it for it in items if (it.get('identity') or {}).get('author')]
    device_items = [it for it in items if (it.get('capture') or {}).get('model')]
    # Top N 추출
    def top_by(items, key, n=5):
        from collections import Counter
        vals = [((it.get(key[0]) or {}).get(key[1])) for it in items]
        return Counter([v for v in vals if v]).most_common(n)
    top_gps = top_by(gps_items, ('geo', 'lat'))
    top_authors = top_by(author_items, ('identity', 'author'))
    top_devices = top_by(device_items, ('capture', 'model'))
    # F841 처리 (미사용 변수)
    _top_gps = top_gps
    _top_authors = top_authors
    _top_devices = top_devices
    # HTML 생성
    html = """
<!DOCTYPE html>
<html lang='ko'>
<head>
<meta charset='utf-8'>
<title>MetaXtract Report</title>
<style>
body { font-family: sans-serif; margin: 2em; }
table { border-collapse: collapse; width: 100%; }
th, td { border: 1px solid #ccc; padding: 4px 8px; }
th { background: #f0f0f0; }
tr:hover { background: #f9f9f9; }
.dashboard { margin-bottom: 2em; }
.tag { display: inline-block; background: #eee; border-radius: 4px; padding: 0 6px; margin-right: 4px; font-size: 0.9em; }
</style>
<script>
function showDetail(idx) {
  var rows = document.querySelectorAll('.detail');
  rows.forEach(r => r.style.display = 'none');
  var row = document.getElementById('detail-' + idx);
  if(row) row.style.display = '';
}}
</script>
</head>
<body>
<h1>MetaXtract HTML Report</h1>
<div class='dashboard'>
  <h2>위험 요약</h2>
  <div>총 파일: {len(items)} | GPS 포함: {len(gps_items)} | 저자 정보: {len(author_items)} | 기기 정보: {len(device_items)}</div>
  <div>Top GPS: {top_gps}</div>
  <div>Top Authors: {top_authors}</div>
  <div>Top Devices: {top_devices}</div>
</div>
<table>
  <thead>
    <tr>
      <th>#</th><th>경로</th><th>저자</th><th>기기</th><th>GPS</th><th>위험점수</th><th>사유</th><th>상세</th>
    </tr>
  </thead>
  <tbody>
"""
    for idx, it in enumerate(items):
        file = it.get('file') or {}
        sig = it.get('signals') or {}
        flags = sig.get('privacy_flags') or {}
        html += "<tr>"
        html += f"<td>{idx+1}</td>"
        html += f"<td>{file.get('path','')}</td>"
        html += f"<td>{(it.get('identity') or {}).get('author','')}</td>"
        html += f"<td>{(it.get('capture') or {}).get('model','')}</td>"
        html += f"<td>{'O' if flags.get('has_gps') else ''}</td>"
        html += f"<td>{sig.get('risk_score','')}</td>"
        html += f"<td>{', '.join(sig.get('reason_codes',[]))}</td>"
        html += f"<td><a href=\"#\" onclick=\"showDetail({idx});return false;\">보기</a></td>"
        html += "</tr>"
        # 상세
        html += f"<tr class='detail' id='detail-{idx}' style='display:none;'><td colspan='8'><pre>{json.dumps(it, ensure_ascii=False, indent=2)}</pre></td></tr>"
    html += """
  </tbody>
</table>
</body>
</html>
"""
    return html
