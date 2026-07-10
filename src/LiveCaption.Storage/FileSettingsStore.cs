using System.Text.Json;
using LiveCaption.Core;

namespace LiveCaption.Storage;

public sealed class FileSettingsStore : ISettingsStore
{
    private static readonly JsonSerializerOptions SerializerOptions = new()
    {
        WriteIndented = true,
        PropertyNameCaseInsensitive = true
    };

    private readonly string _path;

    public FileSettingsStore(string? rootDirectory = null)
    {
        rootDirectory ??= Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LiveCaption");
        _path = Path.Combine(rootDirectory, "settings.json");
    }

    public async Task<AppSettings> LoadAsync(CancellationToken cancellationToken)
    {
        if (!File.Exists(_path))
        {
            return new AppSettings();
        }

        await using var stream = File.OpenRead(_path);
        var stored = await JsonSerializer.DeserializeAsync<AppSettings>(stream, SerializerOptions, cancellationToken).ConfigureAwait(false);
        return Migrate(stored ?? new AppSettings());
    }

    public async Task SaveAsync(AppSettings settings, CancellationToken cancellationToken)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_path)!);
        var temporaryPath = _path + ".tmp";
        await using (var stream = File.Create(temporaryPath))
        {
            await JsonSerializer.SerializeAsync(stream, settings with { SchemaVersion = AppSettings.CurrentSchemaVersion }, SerializerOptions, cancellationToken).ConfigureAwait(false);
        }

        File.Move(temporaryPath, _path, true);
    }

    internal static AppSettings Migrate(AppSettings settings) => settings.SchemaVersion switch
    {
        AppSettings.CurrentSchemaVersion => settings,
        _ => settings with { SchemaVersion = AppSettings.CurrentSchemaVersion }
    };
}
