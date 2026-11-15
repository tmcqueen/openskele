using System.Collections.Concurrent;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization.Metadata;
using Skele.Sdk.Protocol;

namespace Skele.Sdk;

public static class SkelePlugin
{
    /// <summary>Connects to the host over stdio and serves the plugin until the host goes away.</summary>
    public static Task RunAsync(IPlugin plugin, CancellationToken cancellation = default)
    {
        var input = Console.OpenStandardInput();
        var output = Console.OpenStandardOutput();
        // stdout belongs to the protocol; keep stray Console.WriteLine calls off it.
        Console.SetOut(Console.Error);
        return new PluginRuntime(plugin, new Transport(input, output)).RunAsync(cancellation);
    }
}

internal sealed class PluginRuntime(IPlugin plugin, Transport transport)
    : IHost, ICommands, IViews, IWindow, ITerminal, IConfig, IDocuments
{
    private static readonly ProtocolJsonContext Json = ProtocolJsonContext.Default;

    private readonly ConcurrentDictionary<string, CommandHandler> _commands = new();
    private readonly ConcurrentDictionary<string, ITreeDataProvider> _providers = new();
    private readonly ConcurrentDictionary<long, TaskCompletionSource<JsonElement>> _pending = new();
    private long _nextId;

    public string PluginId { get; private set; } = "";
    public string? WorkspaceFolder { get; private set; }
    public ICommands Commands => this;
    public IViews Views => this;
    public IWindow Window => this;
    public ITerminal Terminal => this;
    public IConfig Config => this;
    public IDocuments Documents => this;

    public event Action<IReadOnlyDictionary<string, JsonElement>>? Changed;
    public event Action<DocumentDidSaveParams>? Saved;

    public async Task RunAsync(CancellationToken cancellation)
    {
        while (await transport.ReadAsync(cancellation) is { } frame)
        {
            using var document = JsonDocument.Parse(frame);
            var message = document.RootElement;
            var id = message.TryGetProperty("id", out var rawId) ? rawId.Clone() : (JsonElement?)null;

            if (!message.TryGetProperty("method", out var method))
            {
                Complete(id, message);
                continue;
            }

            var name = method.GetString() ?? "";
            var parameters = message.TryGetProperty("params", out var raw) && raw.ValueKind == JsonValueKind.Object
                ? raw.Clone()
                : JsonSerializer.SerializeToElement(new Empty(), Json.Empty);

            if (id is { } requestId)
            {
                // Requests run concurrently so a slow command does not block the others.
                _ = Task.Run(() => AnswerAsync(requestId, name, parameters), cancellation);
            }
            else if (name == Methods.Host.Exit)
            {
                return;
            }
            else
            {
                Notified(name, parameters);
            }
        }
    }

    // ---- host -> plugin ----------------------------------------------------

    private async Task AnswerAsync(JsonElement id, string method, JsonElement parameters)
    {
        try
        {
            var result = await HandleAsync(method, parameters);
            await SendAsync(writer =>
            {
                writer.WritePropertyName("id");
                id.WriteTo(writer);
                writer.WritePropertyName("result");
                if (result is null)
                {
                    writer.WriteNullValue();
                }
                else
                {
                    result.WriteTo(writer);
                }
            });
        }
        catch (Exception error)
        {
            var code = error is RpcException rpc ? rpc.Code : RpcException.InternalError;
            await SendAsync(writer =>
            {
                writer.WritePropertyName("id");
                id.WriteTo(writer);
                writer.WriteStartObject("error");
                writer.WriteNumber("code", code);
                writer.WriteString("message", error.Message);
                writer.WriteEndObject();
            });
        }
    }

    private async Task<JsonNode?> HandleAsync(string method, JsonElement parameters)
    {
        switch (method)
        {
            case Methods.Host.Initialize:
            {
                var request = Read(parameters, Json.InitializeParams);
                PluginId = request.PluginId;
                WorkspaceFolder = request.WorkspaceFolder;
                await plugin.ActivateAsync(this);
                var reply = new InitializeResult
                {
                    ProtocolVersion = ProtocolInfo.Version,
                    Plugin = new PeerInfo { Name = plugin.Name, Version = plugin.Version },
                };
                return JsonSerializer.SerializeToNode(reply, Json.InitializeResult);
            }
            case Methods.Host.Shutdown:
                await plugin.DeactivateAsync();
                return JsonSerializer.SerializeToNode(new Empty(), Json.Empty);
            case Methods.Host.CommandExecute:
            {
                var request = Read(parameters, Json.CommandExecuteParams);
                if (!_commands.TryGetValue(request.Command, out var handler))
                {
                    throw new RpcException(RpcException.MethodNotFound, $"no handler registered for command {request.Command}");
                }

                return await handler(request.Args ?? []);
            }
            case Methods.Host.ViewGetChildren:
            {
                var request = Read(parameters, Json.ViewGetChildrenParams);
                if (!_providers.TryGetValue(request.ViewId, out var provider))
                {
                    throw new RpcException(RpcException.MethodNotFound, $"no data provider registered for view {request.ViewId}");
                }

                var reply = new ViewGetChildrenResult { Items = await provider.GetChildrenAsync(request.Parent) };
                return JsonSerializer.SerializeToNode(reply, Json.ViewGetChildrenResult);
            }
            default:
                throw new RpcException(RpcException.MethodNotFound, $"method not found: {method}");
        }
    }

    private void Notified(string method, JsonElement parameters)
    {
        try
        {
            switch (method)
            {
                case Methods.Host.ConfigDidChange:
                    Changed?.Invoke(Read(parameters, Json.ConfigDidChangeParams).Changes);
                    break;
                case Methods.Host.DocumentDidSave:
                    Saved?.Invoke(Read(parameters, Json.DocumentDidSaveParams));
                    break;
            }
        }
        catch (Exception error)
        {
            Console.Error.WriteLine($"{method} listener failed: {error}");
        }
    }

    private void Complete(JsonElement? id, JsonElement message)
    {
        if (id is not { ValueKind: JsonValueKind.Number } number || !_pending.TryRemove(number.GetInt64(), out var waiter))
        {
            return;
        }

        if (message.TryGetProperty("error", out var error))
        {
            var code = error.TryGetProperty("code", out var rawCode) ? rawCode.GetInt32() : RpcException.InternalError;
            var text = error.TryGetProperty("message", out var rawText) ? rawText.GetString() : null;
            waiter.SetException(new RpcException(code, text ?? "request failed"));
        }
        else
        {
            waiter.SetResult(message.TryGetProperty("result", out var result) ? result.Clone() : default);
        }
    }

    private static T Read<T>(JsonElement parameters, JsonTypeInfo<T> type)
    {
        try
        {
            return parameters.Deserialize(type) ?? throw new JsonException("expected an object");
        }
        catch (JsonException error)
        {
            throw new RpcException(RpcException.InvalidParams, $"invalid params: {error.Message}");
        }
    }

    // ---- plugin -> host ----------------------------------------------------

    void ICommands.Register(string id, CommandHandler handler) => _commands[id] = handler;

    void IViews.RegisterTreeDataProvider(string viewId, ITreeDataProvider provider) => _providers[viewId] = provider;

    void IViews.Refresh(string viewId) =>
        Notify(Methods.Plugin.ViewRefresh, new ViewRefreshParams { ViewId = viewId }, Json.ViewRefreshParams);

    void IWindow.ShowMessage(string message, MessageLevel level) =>
        Notify(Methods.Plugin.WindowShowMessage, new ShowMessageParams { Level = level, Message = message }, Json.ShowMessageParams);

    void ITerminal.Write(string data, string? terminal) =>
        Notify(Methods.Plugin.TerminalWrite, new TerminalWriteParams { Data = data, Terminal = terminal }, Json.TerminalWriteParams);

    Task<JsonElement> IConfig.GetAsync(string key) =>
        RequestAsync(Methods.Plugin.ConfigGet, new ConfigGetParams { Key = key }, Json.ConfigGetParams);

    async Task<T> IConfig.GetAsync<T>(string key)
    {
        var value = await ((IConfig)this).GetAsync(key);
        object converted = typeof(T) switch
        {
            var t when t == typeof(string) => value.GetString() ?? "",
            var t when t == typeof(bool) => value.GetBoolean(),
            var t when t == typeof(int) => value.GetInt32(),
            var t when t == typeof(long) => value.GetInt64(),
            var t when t == typeof(double) => value.GetDouble(),
            _ => throw new NotSupportedException($"Config.GetAsync<{typeof(T).Name}> is not supported; read the JsonElement instead."),
        };
        return (T)converted;
    }

    private void Notify<T>(string method, T parameters, JsonTypeInfo<T> type)
    {
        // Sends are serialized by the transport, so issuing them in order keeps them in order.
        var send = SendAsync(writer =>
        {
            writer.WriteString("method", method);
            writer.WritePropertyName("params");
            JsonSerializer.Serialize(writer, parameters, type);
        });
        send.ContinueWith(
            failed => Console.Error.WriteLine($"{method} could not be sent: {failed.Exception?.GetBaseException().Message}"),
            TaskContinuationOptions.OnlyOnFaulted);
    }

    private async Task<JsonElement> RequestAsync<T>(string method, T parameters, JsonTypeInfo<T> type)
    {
        var id = Interlocked.Increment(ref _nextId);
        var waiter = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        _pending[id] = waiter;
        await SendAsync(writer =>
        {
            writer.WriteNumber("id", id);
            writer.WriteString("method", method);
            writer.WritePropertyName("params");
            JsonSerializer.Serialize(writer, parameters, type);
        });
        return await waiter.Task;
    }

    private Task SendAsync(Action<Utf8JsonWriter> body)
    {
        var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer))
        {
            writer.WriteStartObject();
            writer.WriteString("jsonrpc", "2.0");
            body(writer);
            writer.WriteEndObject();
        }

        return transport.WriteAsync(buffer.GetBuffer().AsMemory(0, (int)buffer.Length), CancellationToken.None);
    }
}
