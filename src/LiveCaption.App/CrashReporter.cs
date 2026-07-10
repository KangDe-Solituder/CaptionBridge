using System.Diagnostics;
using System.IO;
using System.Text;

namespace LiveCaption.App;

public static class CrashReporter
{
    private static readonly object Sync = new();
    private static readonly string RootDirectory = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LiveCaption", "logs");

    public static string LatestLogPath => Path.Combine(RootDirectory, "latest.log");
    public static string LogDirectory => RootDirectory;

    public static void WriteLifecycle(string message) => Write("LIFECYCLE", message, null);

    public static string WriteException(Exception exception, string context, bool fatal = false)
    {
        Write(fatal ? "FATAL" : "ERROR", context, exception);
        return LatestLogPath;
    }

    public static void OpenLogDirectory()
    {
        Directory.CreateDirectory(RootDirectory);
        Process.Start(new ProcessStartInfo("explorer.exe", RootDirectory) { UseShellExecute = true });
    }

    private static void Write(string level, string message, Exception? exception)
    {
        try
        {
            lock (Sync)
            {
                Directory.CreateDirectory(RootDirectory);
                var builder = new StringBuilder()
                    .Append(DateTimeOffset.Now.ToString("O", System.Globalization.CultureInfo.InvariantCulture))
                    .Append(" [").Append(level).Append("] ").AppendLine(message)
                    .Append("Process: ").Append(Environment.ProcessId)
                    .Append(" | .NET: ").Append(Environment.Version)
                    .Append(" | OS: ").AppendLine(Environment.OSVersion.ToString());
                if (exception is not null)
                {
                    builder.AppendLine(exception.ToString());
                }

                builder.AppendLine(new string('-', 88));
                File.AppendAllText(LatestLogPath, builder.ToString(), Encoding.UTF8);
            }
        }
        catch
        {
            // Diagnostics must never become another reason for the app to terminate.
        }
    }
}
