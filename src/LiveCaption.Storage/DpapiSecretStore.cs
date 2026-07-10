using System.Security.Cryptography;
using System.Text;
using LiveCaption.Core;

namespace LiveCaption.Storage;

public sealed class DpapiSecretStore : ISecretStore
{
    private readonly string _rootDirectory;

    public DpapiSecretStore(string? rootDirectory = null)
    {
        _rootDirectory = rootDirectory ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LiveCaption", "secrets");
    }

    public Task<string?> GetAsync(string name, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var path = GetPath(name);
        if (!File.Exists(path))
        {
            return Task.FromResult<string?>(null);
        }

        var cipher = File.ReadAllBytes(path);
        var plain = ProtectedData.Unprotect(cipher, Entropy(name), DataProtectionScope.CurrentUser);
        return Task.FromResult<string?>(Encoding.UTF8.GetString(plain));
    }

    public Task SetAsync(string name, string value, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        Directory.CreateDirectory(_rootDirectory);
        var cipher = ProtectedData.Protect(Encoding.UTF8.GetBytes(value), Entropy(name), DataProtectionScope.CurrentUser);
        File.WriteAllBytes(GetPath(name), cipher);
        return Task.CompletedTask;
    }

    public Task DeleteAsync(string name, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var path = GetPath(name);
        if (File.Exists(path))
        {
            File.Delete(path);
        }

        return Task.CompletedTask;
    }

    private string GetPath(string name) => Path.Combine(_rootDirectory, $"{name}.bin");

    private static byte[] Entropy(string name) => Encoding.UTF8.GetBytes($"LiveCaption:{name}:v1");
}
