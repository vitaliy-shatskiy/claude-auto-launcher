// Win32 console input for the claude-auto menu: keyboard, MOUSE and resize off one queue.
//
// WHY THIS EXISTS AS C# AND NOT AS PowerShell Add-Type text.
// .NET's Console.ReadKey can never see the mouse - its Windows implementation filters every input
// record through IsReadKeyEvent, whose first check rejects anything that is not a KEY_EVENT, so
// MOUSE_EVENT records are read off the queue and discarded. Worse, Console.KeyAvailable silently
// DEQUEUES a mouse record while scanning for a key (measured: buffer 1 unread -> 0), so a hybrid
// loop that keeps the old poll for the keyboard would lose mouse events to its own keyboard check.
// The read path has to be ReadConsoleInput, wholesale.
//
// WHY PRECOMPILED. Measured 2026-08-15 in a fresh pwsh: Add-Type from source costs 184 ms, loading
// a prebuilt assembly with Add-Type -Path costs 31 ms. The launcher is 967 ms end to end, so
// compiling this text at every start would give back a sixth of it for nothing. Input.ps1 builds
// this file to a DLL once and loads that afterwards; *.dll is gitignored, which is exactly why the
// SOURCE lives here and is committed.
//
// STRUCT RULES, each paid for in the prototype:
//   - Every struct is fully blittable. Win32 BOOL is 4 bytes (int, never bool) and WCHAR is 2
//     (ushort, never char), or array marshalling silently reinterprets field widths.
//   - INPUT_RECORD is a WORD followed by a 4-byte-aligned union; the explicit offsets are load
//     bearing, not decoration.
//
// VERIFIED IN WINDOWS TERMINAL 2026-08-15, not merely in conhost: 5 325 moves, 134 button events,
// 92 wheel events, a double-click and 8 WINDOW_BUFFER_SIZE_EVENTs arrived through ConPTY, with the
// console mode restored bit-exact on exit. WT reports a wheel delta of +-128, NOT the classic
// +-120, so callers must read the SIGN and treat the magnitude as one notch or more.
using System;
using System.Runtime.InteropServices;

namespace ClaudeAuto
{
    public static class ConsoleInput
    {
        public const uint GENERIC_READ = 0x80000000;
        public const uint GENERIC_WRITE = 0x40000000;
        public const uint FILE_SHARE_READ = 0x1;
        public const uint FILE_SHARE_WRITE = 0x2;
        public const uint OPEN_EXISTING = 3;

        public const uint ENABLE_PROCESSED_INPUT = 0x0001;
        public const uint ENABLE_LINE_INPUT = 0x0002;
        public const uint ENABLE_ECHO_INPUT = 0x0004;
        public const uint ENABLE_WINDOW_INPUT = 0x0008;
        public const uint ENABLE_MOUSE_INPUT = 0x0010;
        public const uint ENABLE_INSERT_MODE = 0x0020;
        public const uint ENABLE_QUICK_EDIT_MODE = 0x0040;
        public const uint ENABLE_EXTENDED_FLAGS = 0x0080;

        public const ushort KEY_EVENT = 0x0001;
        public const ushort MOUSE_EVENT = 0x0002;
        public const ushort WINDOW_BUFFER_SIZE_EVENT = 0x0004;

        public const uint FROM_LEFT_1ST_BUTTON_PRESSED = 0x0001;
        public const uint RIGHTMOST_BUTTON_PRESSED = 0x0002;
        public const uint FROM_LEFT_2ND_BUTTON_PRESSED = 0x0004;

        public const uint MOUSE_MOVED = 0x0001;
        public const uint DOUBLE_CLICK = 0x0002;
        public const uint MOUSE_WHEELED = 0x0004;
        public const uint MOUSE_HWHEELED = 0x0008;

        public const uint SHIFT_PRESSED = 0x0010;
        // Caps Lock is a STATE, not a modifier: with it on, the u key reports 'U' and no Shift.
        // The hotkey matcher needs it to tell that real keypress from a mouse report's coordinate
        // byte, which is also an uppercase character and carries no such flag.
        public const uint CAPSLOCK_ON = 0x0080;
        public const uint LEFT_ALT_PRESSED = 0x0002;
        public const uint RIGHT_ALT_PRESSED = 0x0001;
        public const uint LEFT_CTRL_PRESSED = 0x0008;
        public const uint RIGHT_CTRL_PRESSED = 0x0004;

        public const uint WAIT_OBJECT_0 = 0x00000000;
        public const uint WAIT_TIMEOUT = 0x00000102;

        [StructLayout(LayoutKind.Sequential)]
        public struct COORD { public short X; public short Y; }

        [StructLayout(LayoutKind.Sequential)]
        public struct KEY_EVENT_RECORD
        {
            public int bKeyDown;            // Win32 BOOL is 4 bytes
            public ushort wRepeatCount;
            public ushort wVirtualKeyCode;
            public ushort wVirtualScanCode;
            public ushort UnicodeChar;      // WCHAR is 2 bytes
            public uint dwControlKeyState;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct MOUSE_EVENT_RECORD
        {
            public COORD dwMousePosition;
            public uint dwButtonState;
            public uint dwControlKeyState;
            public uint dwEventFlags;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct WINDOW_BUFFER_SIZE_RECORD { public COORD dwSize; }

        [StructLayout(LayoutKind.Explicit)]
        public struct INPUT_RECORD
        {
            [FieldOffset(0)] public ushort EventType;
            [FieldOffset(4)] public KEY_EVENT_RECORD KeyEvent;
            [FieldOffset(4)] public MOUSE_EVENT_RECORD MouseEvent;
            [FieldOffset(4)] public WINDOW_BUFFER_SIZE_RECORD WindowBufferSizeEvent;
        }

        // CONIN$ rather than GetStdHandle: the launcher can be started with stdin redirected (the
        // nightly audit does exactly that, feeding it < NUL), and GetConsoleMode on a redirected
        // std handle fails with error 203 while CreateFile("CONIN$") still succeeds. Measured.
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        public static extern IntPtr CreateFileW(string lpFileName, uint dwDesiredAccess, uint dwShareMode,
            IntPtr lpSecurityAttributes, uint dwCreationDisposition, uint dwFlagsAndAttributes, IntPtr hTemplateFile);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool GetConsoleMode(IntPtr hConsoleHandle, out uint lpMode);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool SetConsoleMode(IntPtr hConsoleHandle, uint dwMode);

        [DllImport("kernel32.dll", SetLastError = true, EntryPoint = "ReadConsoleInputW")]
        public static extern bool ReadConsoleInputW(IntPtr hConsoleInput, [Out] INPUT_RECORD[] lpBuffer,
            uint nLength, out uint lpNumberOfEventsRead);

        // Only the tests use this: it is the one way to exercise the reader without a human hand on
        // the mouse, and it is what proved the struct marshalling end to end.
        [DllImport("kernel32.dll", SetLastError = true, EntryPoint = "WriteConsoleInputW")]
        public static extern bool WriteConsoleInputW(IntPtr hConsoleInput, INPUT_RECORD[] lpBuffer,
            uint nLength, out uint lpNumberOfEventsWritten);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool GetNumberOfConsoleInputEvents(IntPtr hConsoleInput, out uint lpNumberOfEvents);

        // Look at the next record WITHOUT consuming it. Required to tell a bare Escape from the
        // start of a VT sequence: the decision needs the byte after ESC, and reading it to find out
        // would eat a real keypress when the answer is "nothing follows".
        [DllImport("kernel32.dll", SetLastError = true, EntryPoint = "PeekConsoleInputW")]
        public static extern bool PeekConsoleInputW(IntPtr hConsoleInput, [Out] INPUT_RECORD[] lpBuffer,
            uint nLength, out uint lpNumberOfEventsRead);

        // Throws away everything queued while a child process owned the screen. Read-and-discard is
        // NOT equivalent: the reader converts a key-up and a bare modifier to null, so a loop that
        // stops on the first null would leave records behind. This drops them all in one call.
        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool FlushConsoleInputBuffer(IntPtr hConsoleInput);

        // The input handle is waitable and signals exactly when the queue is non-empty, so the menu
        // keeps its periodic tick (and therefore its resize handling and its Ctrl+C responsiveness)
        // without ever blocking in native code. Proven event-driven, not lucky polling: five
        // consecutive WAIT_TIMEOUTs with no input, then an injected event woke it at once.
        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern uint WaitForSingleObject(IntPtr hHandle, uint dwMilliseconds);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool CloseHandle(IntPtr hObject);

        // The wheel amount is a SIGNED 16-bit value in the high word of dwButtonState. Reading it
        // as unsigned makes every scroll look like "up".
        public static short WheelDelta(uint buttonState)
        {
            return unchecked((short)(ushort)(buttonState >> 16));
        }
    }
}
