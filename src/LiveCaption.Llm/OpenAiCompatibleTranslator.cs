using System.Diagnostics;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using LiveCaption.Core;

namespace LiveCaption.Llm;

public sealed class OpenAiCompatibleTranslator : ITranslator
{
    public const string ApiKeySecretName = "primary-api-key";

    private readonly HttpClient _httpClient;
    private readonly ISecretStore _secretStore;

    public OpenAiCompatibleTranslator(HttpClient httpClient, ISecretStore secretStore)
    {
        _httpClient = httpClient;
        _secretStore = secretStore;
    }

    public async Task<TranslationResult> TranslateAsync(TranslationRequest request, CancellationToken cancellationToken)
    {
        var storedApiKey = await _secretStore.GetAsync(ApiKeySecretName, cancellationToken).ConfigureAwait(false);
        if (!TryNormalizeApiKey(storedApiKey, out var apiKey, out var apiKeyError))
        {
            return new TranslationResult(string.Empty, 0, request.ProviderOptions.Model, true, apiKeyError);
        }

        try
        {
            var stopwatch = Stopwatch.StartNew();
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            timeout.CancelAfter(request.ProviderOptions.TimeoutMilliseconds);
            using var message = new HttpRequestMessage(HttpMethod.Post, ResolveEndpoint(request.ProviderOptions.Endpoint));
            message.Headers.Authorization = new AuthenticationHeaderValue("Bearer", apiKey);
            message.Content = new StringContent(CreatePayload(request).ToJsonString(), Encoding.UTF8, "application/json");

            using var response = await _httpClient.SendAsync(message, timeout.Token).ConfigureAwait(false);
            var content = await response.Content.ReadAsStringAsync(timeout.Token).ConfigureAwait(false);
            stopwatch.Stop();
            if (!response.IsSuccessStatusCode)
            {
                return new TranslationResult(string.Empty, stopwatch.ElapsedMilliseconds, request.ProviderOptions.Model, true,
                    $"API 返回 {(int)response.StatusCode}: {ExtractError(content)}");
            }

            var translated = ExtractContent(content);
            return string.IsNullOrWhiteSpace(translated)
                ? new TranslationResult(string.Empty, stopwatch.ElapsedMilliseconds, request.ProviderOptions.Model, true, "API 未返回可用译文。")
                : new TranslationResult(translated.Trim(), stopwatch.ElapsedMilliseconds, request.ProviderOptions.Model);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return new TranslationResult(string.Empty, request.ProviderOptions.TimeoutMilliseconds, request.ProviderOptions.Model, true, "翻译请求超时。");
        }
        catch (Exception exception)
        {
            return new TranslationResult(string.Empty, 0, request.ProviderOptions.Model, true, exception.Message);
        }
    }

    public static JsonObject CreatePayload(TranslationRequest request)
    {
        var (system, user) = PromptBuilder.Build(request);
        var root = new JsonObject
        {
            ["model"] = request.ProviderOptions.Model,
            ["messages"] = new JsonArray(
                new JsonObject { ["role"] = "system", ["content"] = system },
                new JsonObject { ["role"] = "user", ["content"] = user }),
            ["stream"] = false,
            ["max_tokens"] = request.ProviderOptions.MaxTokens,
            ["temperature"] = request.ProviderOptions.Temperature
        };

        if (!string.IsNullOrWhiteSpace(request.ProviderOptions.ExtraBodyJson))
        {
            var extra = JsonNode.Parse(request.ProviderOptions.ExtraBodyJson) as JsonObject
                        ?? throw new JsonException("extra_body 必须是 JSON 对象。");
            Merge(root, extra);
        }

        return root;
    }

    public static string ExtractContent(string responseJson)
    {
        using var document = JsonDocument.Parse(responseJson);
        return document.RootElement
            .GetProperty("choices")[0]
            .GetProperty("message")
            .GetProperty("content")
            .GetString() ?? string.Empty;
    }

    public static bool TryNormalizeApiKey(
        string? value,
        out string normalized,
        out string? error)
    {
        normalized = string.Empty;
        error = null;

        if (string.IsNullOrWhiteSpace(value))
        {
            error = "请先在设置中填写 API Key。";
            return false;
        }

        var candidate = TrimSurroundingQuotes(RemoveFormattingCharacters(value).Trim());
        if (candidate.StartsWith("Bearer", StringComparison.OrdinalIgnoreCase) &&
            candidate.Length > "Bearer".Length &&
            char.IsWhiteSpace(candidate["Bearer".Length]))
        {
            candidate = candidate["Bearer".Length..].Trim();
        }

        candidate = TrimSurroundingQuotes(RemoveWhitespaceAndFormattingCharacters(candidate));
        if (candidate.Length == 0)
        {
            error = "请先在设置中填写 API Key。";
            return false;
        }

        for (var index = 0; index < candidate.Length; index++)
        {
            var character = candidate[index];
            if (character is < (char)0x21 or > (char)0x7E)
            {
                error = $"API Key 的第 {index + 1} 个字符为 U+{(int)character:X4}，不能用于 HTTP 请求头。"
                    + " 已自动清理常见的空格、引号和不可见格式字符；请确认粘贴的是密钥本身。";
                return false;
            }
        }

        normalized = candidate;
        return true;
    }

    private static Uri ResolveEndpoint(string endpoint)
    {
        var normalized = endpoint.Trim().TrimEnd('/');
        if (!normalized.EndsWith("/chat/completions", StringComparison.OrdinalIgnoreCase))
        {
            normalized += "/chat/completions";
        }

        return new Uri(normalized, UriKind.Absolute);
    }

    private static string TrimSurroundingQuotes(string value) =>
        value.Trim('"', '\'', '\u201c', '\u201d', '\u2018', '\u2019');

    private static string RemoveFormattingCharacters(string value)
    {
        var builder = new StringBuilder(value.Length);
        foreach (var character in value)
        {
            if (character is '\u200b' or '\u200c' or '\u200d' or '\u2060' or '\ufeff')
            {
                continue;
            }

            builder.Append(character);
        }

        return builder.ToString();
    }

    private static string RemoveWhitespaceAndFormattingCharacters(string value)
    {
        var builder = new StringBuilder(value.Length);
        foreach (var character in value)
        {
            if (char.IsWhiteSpace(character) ||
                character is '\u200b' or '\u200c' or '\u200d' or '\u2060' or '\ufeff')
            {
                continue;
            }

            builder.Append(character);
        }

        return builder.ToString();
    }

    private static void Merge(JsonObject destination, JsonObject source)
    {
        foreach (var (key, value) in source)
        {
            if (value is JsonObject sourceObject && destination[key] is JsonObject destinationObject)
            {
                Merge(destinationObject, sourceObject);
            }
            else
            {
                destination[key] = value?.DeepClone();
            }
        }
    }

    private static string ExtractError(string content)
    {
        try
        {
            using var document = JsonDocument.Parse(content);
            return document.RootElement.TryGetProperty("error", out var error)
                ? error.ToString()
                : content;
        }
        catch (JsonException)
        {
            return content;
        }
    }
}
