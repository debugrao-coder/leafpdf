from pathlib import Path
from reportlab.pdfgen import canvas
from reportlab.lib.pagesizes import A4

root=Path(__file__).resolve().parents[1]/'test-output'
root.mkdir(exist_ok=True)
c=canvas.Canvas(str(root/'多页阅读测试.pdf'),pagesize=A4)
for i in range(1,8):
    c.setFillColorRGB(.12,.17,.24)
    c.setFont('Helvetica-Bold',28)
    c.drawString(48,780,f'LeafPDF / Reading Test {i:02}')
    c.setFillColorRGB(.3,.4,.55)
    c.setFont('Helvetica',13)
    c.drawString(48,745,'Native Windows reader. Search, zoom and page layout.')
    for j in range(16):
        c.setFont('Helvetica',11)
        c.drawString(48,700-j*27,f'Page {i}. Line {j+1}. The quick brown fox explores a quiet library.')
    c.setFillColorRGB(.31,.46,.85)
    c.roundRect(48,120,500,68,12,fill=1,stroke=0)
    c.setFillColorRGB(1,1,1)
    c.drawString(68,149,'A calm workspace for your documents.')
    c.showPage()
c.save()
(root/'invalid.pdf').write_text('not a pdf')
