// Records a notch window (frames + window-region box per frame) while scripted input runs.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

public static class NotchRec
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowW(string c, IntPtr n);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern int GetWindowRgn(IntPtr h, IntPtr rgn);
    [DllImport("gdi32.dll")] static extern IntPtr CreateRectRgn(int a, int b, int c, int d);
    [DllImport("gdi32.dll")] static extern int GetRgnBox(IntPtr rgn, out RECT r);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr o);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
    public struct RECT { public int L, T, R, B; }

    public static IntPtr Hwnd;
    public static RECT Win;
    static Thread worker;
    static volatile bool running;
    static List<Bitmap> frames = new List<Bitmap>();
    static List<string> log = new List<string>();

    public static bool Find(string cls)
    {
        Hwnd = FindWindowW(cls, IntPtr.Zero);
        if (Hwnd == IntPtr.Zero) return false;
        GetWindowRect(Hwnd, out Win);
        return true;
    }

    static string RgnBox()
    {
        IntPtr r = CreateRectRgn(0, 0, 0, 0);
        int kind = GetWindowRgn(Hwnd, r);
        RECT b;
        GetRgnBox(r, out b);
        DeleteObject(r);
        return kind == 0 ? "none" : b.L + "," + b.T + "," + b.R + "," + b.B;
    }

    public static void Start(int fps)
    {
        frames.Clear(); log.Clear();
        running = true;
        worker = new Thread(() =>
        {
            var sw = Stopwatch.StartNew();
            int w = Win.R - Win.L, h = Win.B - Win.T;
            double period = 1000.0 / fps;
            long n = 0;
            while (running)
            {
                var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb);
                using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(Win.L, Win.T, 0, 0, new Size(w, h));
                frames.Add(bmp);
                log.Add(sw.Elapsed.TotalMilliseconds.ToString("F1") + ";" + RgnBox());
                n++;
                double next = n * period - sw.Elapsed.TotalMilliseconds;
                if (next > 1) Thread.Sleep((int)next);
            }
        });
        worker.Start();
    }

    public static int Stop(string dir, int keepEvery)
    {
        running = false;
        worker.Join();
        Directory.CreateDirectory(dir);
        int saved = 0;
        for (int i = 0; i < frames.Count; i++)
        {
            if (i % keepEvery == 0 || i == frames.Count - 1)
            {
                frames[i].Save(Path.Combine(dir, i.ToString("D4") + ".png"), ImageFormat.Png);
                saved++;
            }
            frames[i].Dispose();
        }
        File.WriteAllLines(Path.Combine(dir, "log.csv"), log);
        frames.Clear();
        return saved;
    }

    public static void Shot(string path)
    {
        int w = Win.R - Win.L, h = Win.B - Win.T;
        using (var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb))
        {
            using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(Win.L, Win.T, 0, 0, new Size(w, h));
            bmp.Save(path, ImageFormat.Png);
        }
    }

    // Coordinates are relative to the recorded window.
    public static void Move(int x, int y) { SetCursorPos(Win.L + x, Win.T + y); }
    public static void Click(int x, int y) { Move(x, y); Thread.Sleep(30); mouse_event(2, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(40); mouse_event(4, 0, 0, 0, UIntPtr.Zero); }
    public static void RClick(int x, int y) { Move(x, y); Thread.Sleep(30); mouse_event(8, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(40); mouse_event(16, 0, 0, 0, UIntPtr.Zero); }
    public static void Down() { mouse_event(2, 0, 0, 0, UIntPtr.Zero); }
    public static void Up() { mouse_event(4, 0, 0, 0, UIntPtr.Zero); }
    public static void MoveAbs(int x, int y) { SetCursorPos(x, y); }
    public static void Wheel(int notches, bool ctrl) { if (ctrl) keybd_event(0x11, 0, 0, UIntPtr.Zero); mouse_event(0x0800, 0, 0, unchecked((uint)(notches * 120)), UIntPtr.Zero); Thread.Sleep(20); if (ctrl) keybd_event(0x11, 0, 2, UIntPtr.Zero); }
    public static void Key(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); Thread.Sleep(30); keybd_event(vk, 0, 2, UIntPtr.Zero); }
    public static void Combo(byte a, byte b) { keybd_event(a, 0, 0, UIntPtr.Zero); Thread.Sleep(30); keybd_event(b, 0, 0, UIntPtr.Zero); Thread.Sleep(30); keybd_event(b, 0, 2, UIntPtr.Zero); Thread.Sleep(30); keybd_event(a, 0, 2, UIntPtr.Zero); }
    public static void Type(string s) { foreach (char c in s) { byte vk = (byte)char.ToUpperInvariant(c); if (c == ' ') vk = 0x20; Key(vk); Thread.Sleep(20); } }
}
