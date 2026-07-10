using LiveCaption.Core;
using LiveCaption.Llm;

namespace LiveCaption.App;

public sealed class SettingsService
{
    private readonly ISettingsStore _settingsStore;
    private readonly ISecretStore _secretStore;

    public SettingsService(ISettingsStore settingsStore, ISecretStore secretStore)
    {
        _settingsStore = settingsStore;
        _secretStore = secretStore;
    }

    public AppSettings Current { get; private set; } = new();
    public string ApiKey { get; private set; } = string.Empty;

    public async Task LoadAsync(CancellationToken cancellationToken)
    {
        try
        {
            Current = await _settingsStore.LoadAsync(cancellationToken);
        }
        catch (Exception exception)
        {
            CrashReporter.WriteException(exception, "Loading settings failed; defaults were used.");
            Current = new AppSettings();
        }

        try
        {
            ApiKey = await _secretStore.GetAsync(OpenAiCompatibleTranslator.ApiKeySecretName, cancellationToken) ?? string.Empty;
        }
        catch (Exception exception)
        {
            CrashReporter.WriteException(exception, "Loading the encrypted API key failed; an empty key was used.");
            ApiKey = string.Empty;
        }
    }

    public async Task SaveAsync(AppSettings settings, string apiKey, CancellationToken cancellationToken)
    {
        await _settingsStore.SaveAsync(settings, cancellationToken);
        if (!string.Equals(apiKey, ApiKey, StringComparison.Ordinal))
        {
            if (string.IsNullOrWhiteSpace(apiKey))
            {
                await _secretStore.DeleteAsync(OpenAiCompatibleTranslator.ApiKeySecretName, cancellationToken);
            }
            else
            {
                await _secretStore.SetAsync(OpenAiCompatibleTranslator.ApiKeySecretName, apiKey.Trim(), cancellationToken);
            }
        }

        Current = settings;
        ApiKey = apiKey.Trim();
    }

    public ProviderOptions ProviderOptions => new(
        Current.Translator.Endpoint,
        Current.Translator.Model,
        Current.Translator.ExtraBodyJson,
        Current.Translator.TimeoutMilliseconds,
        Current.Translator.MaxTokens,
        Current.Translator.Temperature);
}
