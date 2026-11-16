using System.Text.Json;
using System.Text.Json.Nodes;
using Skele.Sdk.Protocol;

namespace Skele.Sdk;

/// <summary>A plugin's entry point. Pass an implementation to <see cref="SkelePlugin.RunAsync"/>.</summary>
public interface IPlugin
{
    string Name { get; }

    string Version { get; }

    /// <summary>Runs once, before the host is told the plugin is ready. Register handlers here.</summary>
    Task ActivateAsync(IHost host);

    Task DeactivateAsync() => Task.CompletedTask;
}

/// <summary>Supplies the rows of a view declared in the manifest.</summary>
public interface ITreeDataProvider
{
    /// <summary>Rows of the view, or the children of the row with id <paramref name="parent"/>.</summary>
    Task<IReadOnlyList<TreeItem>> GetChildrenAsync(string? parent);
}

/// <summary>Implements a command. The result is returned to the caller as JSON; null for none.</summary>
public delegate Task<JsonNode?> CommandHandler(IReadOnlyList<JsonElement> args);

/// <summary>
/// Implements a tool. <paramref name="arguments"/> are already checked against the
/// tool's <c>inputSchema</c>. A thrown exception becomes a result with <c>IsError</c>.
/// </summary>
public delegate Task<ToolCallResult> ToolHandler(JsonElement arguments);

/// <summary>Builds tool results.</summary>
public static class ToolResults
{
    /// <summary>A successful result with one piece of text.</summary>
    public static ToolCallResult Text(string text) => new()
    {
        Content = [new ToolContent { Type = ToolContentType.Text, Text = text }],
        IsError = false,
    };

    /// <summary>A failed result whose message the agent reads.</summary>
    public static ToolCallResult Error(string message) => new()
    {
        Content = [new ToolContent { Type = ToolContentType.Text, Text = message }],
        IsError = true,
    };
}

/// <summary>What a plugin can ask of the host.</summary>
public interface IHost
{
    string PluginId { get; }

    string? WorkspaceFolder { get; }

    ICommands Commands { get; }

    ITools Tools { get; }

    IViews Views { get; }

    IWindow Window { get; }

    ITerminal Terminal { get; }

    IConfig Config { get; }

    IDocuments Documents { get; }
}

public interface ICommands
{
    /// <summary>Implements a command declared in the manifest.</summary>
    void Register(string id, CommandHandler handler);
}

public interface ITools
{
    /// <summary>Implements a tool declared in the manifest.</summary>
    void Register(string name, ToolHandler handler);
}

public interface IViews
{
    /// <summary>Supplies the data for a view declared in the manifest.</summary>
    void RegisterTreeDataProvider(string viewId, ITreeDataProvider provider);

    /// <summary>Tells the host the view's data changed so it queries again.</summary>
    void Refresh(string viewId);
}

public interface IWindow
{
    void ShowMessage(string message, MessageLevel level = MessageLevel.Info);
}

public interface ITerminal
{
    /// <summary>Appends to an output terminal; defaults to the plugin's own.</summary>
    void Write(string data, string? terminal = null);
}

public interface IConfig
{
    /// <summary>Effective value of a setting: workspace, then user, then default.</summary>
    Task<JsonElement> GetAsync(string key);

    /// <summary>
    /// Effective value of a setting as <typeparamref name="T"/>, which must be
    /// string, bool, int, long or double. Read other shapes from <see cref="GetAsync(string)"/>.
    /// </summary>
    Task<T> GetAsync<T>(string key);

    /// <summary>Raised with the changed keys and their new effective values.</summary>
    event Action<IReadOnlyDictionary<string, JsonElement>>? Changed;
}

public interface IDocuments
{
    event Action<DocumentDidSaveParams>? Saved;
}

/// <summary>Fails a request with a JSON-RPC error the host can report.</summary>
public sealed class RpcException(int code, string message) : Exception(message)
{
    public const int MethodNotFound = -32601;
    public const int InvalidParams = -32602;
    public const int InternalError = -32603;

    public int Code { get; } = code;
}
