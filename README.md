# Islet

Một "notch" kiểu Dynamic Island cho Windows, viết bằng Rust (Direct2D + WinRT).
Đây là bản viết lại từ đầu, lấy cảm hứng từ chức năng của Nusa. Không có mã hay
tài nguyên nào được sao chép từ Nusa.

## Chạy

```powershell
cd D:\islet
cargo build --release
.\target\release\islet.exe
```

Tắt Nusa trước khi chạy để hai notch không đè lên nhau. Muốn thoát Islet thì
chuột phải vào notch hoặc icon khay, rồi chọn **Exit**.

## Tính năng

| Tính năng | Cách dùng |
|---|---|
| Notch thu gọn | Icon sạc / mascot bên trái, giờ bên phải. Rê chuột thì notch phồng ra, bấm thì mở rộng |
| Nhạc + lời | Phát nhạc ở Spotify, trình duyệt, v.v. Có ảnh bìa, tiến trình, prev/play/next, lời đồng bộ từ LRCLIB |
| Focus (Pomodoro) | 25/5/15 phút, nghỉ dài sau 4 phiên, thông báo kèm âm thanh |
| Uống nước | Mục tiêu 1.5–3.0 L, cỡ chai, khung giờ, Drank/Refilled/Snooze/Reset, nhắc mỗi giờ |
| To-do | Thêm/sửa/xoá/đánh dấu xong, Start (chạy Pomodoro cho task), Later. Cuộn bằng con lăn chuột |
| Shelf | Kéo file hoặc link vào notch, bấm để mở, kéo ra để thả vào app khác |
| Thông báo nhanh | Âm lượng, Caps Lock, đổi ngôn ngữ bàn phím, sạc/rút sạc, pin yếu, mạng |
| Menu khay | Start with Windows, Mascot, Minimal notch, About, Exit |
| Toàn màn hình | Tự ẩn khi app toàn màn hình (game, video) |

Dữ liệu được lưu trong `%APPDATA%\Islet\` (settings/todo/water/shelf.txt) và
`%LOCALAPPDATA%\Islet\Shelf\` (link đã thả vào).
