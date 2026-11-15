// Sample plugin: one command that describes the .NET runtime it runs on, and
// a tree view of the same facts. Everything it shows is data; the host renders it.

using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using Skele.Sdk;
using Skele.Sdk.Protocol;

await SkelePlugin.RunAsync(new RuntimeInfoPlugin());

internal sealed class RuntimeInfoPlugin : IPlugin, ITreeDataProvider
{
    private const string View = "dotnet.runtime";
    private int _described;

    public string Name => "Runtime Info";

    public string Version => "0.1.0";

    public Task ActivateAsync(IHost host)
    {
        host.Commands.Register("dotnet.describe", async _ =>
        {
            var label = await host.Config.GetAsync<string>("dotnet.label");
            var message = $"{label} {RuntimeInformation.FrameworkDescription}";

            _described++;
            host.Terminal.Write($"{message}\r\n");
            host.Window.ShowMessage(message);
            host.Views.Refresh(View);
            return JsonValue.Create(message);
        });

        host.Views.RegisterTreeDataProvider(View, this);
        host.Config.Changed += _ => host.Views.Refresh(View);
        host.Documents.Saved += saved => host.Terminal.Write($"saved {saved.Uri}\r\n");
        return Task.CompletedTask;
    }

    public Task<IReadOnlyList<TreeItem>> GetChildrenAsync(string? parent)
    {
        IReadOnlyList<TreeItem> rows = parent switch
        {
            null =>
            [
                new TreeItem
                {
                    Id = "describe",
                    Label = "Describe runtime",
                    Description = "run dotnet.describe",
                    Icon = "play",
                    Command = new CommandRef { Command = "dotnet.describe" },
                },
                new TreeItem { Id = "facts", Label = "Facts", Collapsible = true },
                new TreeItem { Id = "described", Label = "Described", Description = _described.ToString() },
            ],
            "facts" =>
            [
                Fact("framework", "Framework", RuntimeInformation.FrameworkDescription),
                Fact("os", "OS", RuntimeInformation.OSDescription),
                Fact("arch", "Architecture", RuntimeInformation.ProcessArchitecture.ToString()),
            ],
            _ => [],
        };
        return Task.FromResult(rows);
    }

    private static TreeItem Fact(string id, string label, string value) =>
        new() { Id = $"facts/{id}", Label = label, Description = value, Icon = "gear" };
}
