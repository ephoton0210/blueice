// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

using System.Text.Json;
using System.Runtime.InteropServices.WindowsRuntime;
using BlueIce.WinUI.Protocol;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.System;

namespace BlueIce.WinUI;

public sealed partial class MainWindow : Window
{
    private readonly BrowserSession session = new();
    private readonly Dictionary<ulong, (WriteableBitmap Image, ulong Generation)> frames = new();
    private readonly Dictionary<ulong, string> urls = new();
    private readonly Dictionary<ulong, (bool Back, bool Forward)> history = new();
    private bool ready, changingTabs, closing;
    private ulong? selectedTab;
    private readonly DispatcherTimer resizeTimer = new() { Interval = TimeSpan.FromMilliseconds(100) };

    public MainWindow()
    {
        InitializeComponent();
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1120, 800));
        AppWindow.Closing += async (_, args) =>
        {
            if (closing) return;
            args.Cancel = true;
            closing = true;
            resizeTimer.Stop();
            ready = Navigation.IsEnabled = false;
            await session.DisposeAsync();
            Close();
        };
        Closed += (_, _) => Application.Current.Exit();
        resizeTimer.Tick += async (_, _) => { resizeTimer.Stop(); await ResizePage(); };
        session.Received = Receive;
        session.Failed = message => DispatcherQueue.TryEnqueue(() => { Status.Text = message; Navigation.IsEnabled = ready = false; });
    }

    private async void WindowLoaded(object sender, RoutedEventArgs args)
    {
        try
        {
            string[] arguments = Environment.GetCommandLineArgs();
            int option = Array.IndexOf(arguments, "--core-exe");
            string executable = option >= 0 && option + 1 < arguments.Length ? arguments[option + 1] : Path.Combine(AppContext.BaseDirectory, "blueice-core.exe");
            await session.StartAsync(executable);
            Navigation.IsEnabled = ready = true;
            await session.SendAsync("ListTabs");
            await session.SendAsync(BrowserWire.Command("Navigate", new { url = "about:credits" }), 1);
            await ResizePage();
        }
        catch (Exception error) { EmptyMessage.Text = Status.Text = error.Message; await session.DisposeAsync(); }
    }

    private async Task Receive(JsonElement envelope)
    {
        JsonElement message = envelope.GetProperty("message");
        if (message.ValueKind != JsonValueKind.Object) return;
        JsonProperty variant = message.EnumerateObject().First();
        ulong? tab = envelope.TryGetProperty("tab_id", out var tabValue) ? tabValue.GetUInt64() : null;
        byte[]? pixels = null;
        if (variant.Name == "FrameReady")
        {
            try { pixels = FramePixels.ReadBgra(session.FrameDirectory, variant.Value.GetProperty("shm_path").GetString()!, variant.Value.GetProperty("width").GetInt32(), variant.Value.GetProperty("height").GetInt32()); }
            catch (FileNotFoundException) { return; } // A newer retained generation will follow.
        }
        var completed = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        if (!DispatcherQueue.TryEnqueue(() =>
        {
            try { ApplyMessage(variant.Name, variant.Value, tab, pixels); completed.SetResult(); }
            catch (Exception error) { completed.SetException(error); }
        })) return;
        await completed.Task;
    }

    private void ApplyMessage(string name, JsonElement value, ulong? tab, byte[]? pixels)
    {
        switch (name)
        {
            case "Tabs":
                changingTabs = true;
                Tabs.TabItems.Clear();
                var live = new HashSet<ulong>();
                foreach (JsonElement item in value.EnumerateArray())
                {
                    ulong id = item.GetProperty("id").GetUInt64();
                    live.Add(id);
                    string url = item.GetProperty("url").GetString() ?? "";
                    urls[id] = url;
                    Tabs.TabItems.Add(new TabViewItem { Tag = id, Header = url.Length == 0 ? "New tab" : url, IsClosable = true });
                }
                foreach (ulong stale in urls.Keys.Where(id => !live.Contains(id)).ToArray()) { frames.Remove(stale); urls.Remove(stale); history.Remove(stale); }
                if (selectedTab is null || !live.Contains(selectedTab.Value)) selectedTab = live.Order().Cast<ulong?>().FirstOrDefault();
                Tabs.SelectedItem = Tabs.TabItems.OfType<TabViewItem>().FirstOrDefault(item => (ulong)item.Tag == selectedTab);
                changingTabs = false;
                ShowSelected();
                break;
            case "TabOpened":
                selectedTab = value.GetProperty("tab_id").GetUInt64();
                _ = Send("ListTabs");
                _ = Send(BrowserWire.Command("Navigate", new { url = "about:credits" }), selectedTab);
                break;
            case "TabClosed":
                _ = Send("ListTabs");
                break;
            case "Navigated" when tab is ulong id:
                urls[id] = value.GetProperty("url").GetString() ?? "";
                foreach (var item in Tabs.TabItems.OfType<TabViewItem>()) if ((ulong)item.Tag == id) item.Header = urls[id];
                if (selectedTab == id) { Address.Text = urls[id]; Status.Text = "Ready"; }
                _ = Send("GetHistoryState", id);
                break;
            case "HistoryState" when tab is ulong id:
                history[id] = (value.GetProperty("can_go_back").GetBoolean(), value.GetProperty("can_go_forward").GetBoolean());
                if (selectedTab == id) ShowHistory();
                break;
            case "FrameReady" when tab is ulong id:
                ulong generation = value.GetProperty("generation").GetUInt64();
                if (frames.TryGetValue(id, out var old) && generation <= old.Generation) break;
                var bitmap = new WriteableBitmap(value.GetProperty("width").GetInt32(), value.GetProperty("height").GetInt32());
                using (Stream buffer = bitmap.PixelBuffer.AsStream()) buffer.Write(pixels!);
                bitmap.Invalidate();
                frames[id] = (bitmap, generation);
                if (selectedTab == id) ShowSelected();
                break;
            case "GatekeeperBlocked":
                if (tab == selectedTab) Status.Text = "Navigation blocked: " + value.GetProperty("reason").GetString();
                break;
            case "Error":
                if (tab is null || tab == selectedTab) Status.Text = value.GetProperty("message").GetString();
                break;
        }
    }

    private void ShowSelected()
    {
        PageImage.Source = selectedTab is ulong id && frames.TryGetValue(id, out var frame) ? frame.Image : null;
        EmptyMessage.Visibility = PageImage.Source is null ? Visibility.Visible : Visibility.Collapsed;
        EmptyMessage.Text = "New tab";
        Address.Text = selectedTab is ulong tab && urls.TryGetValue(tab, out string? url) ? url : "";
        ShowHistory();
    }

    private void ShowHistory()
    {
        var state = selectedTab is ulong tab && history.TryGetValue(tab, out var value) ? value : default;
        BackButton.IsEnabled = state.Back;
        ForwardButton.IsEnabled = state.Forward;
    }

    private async Task Send(object command, ulong? tab = null)
    {
        if (!ready) return;
        try { await session.SendAsync(command, tab); }
        catch (Exception error) { Status.Text = error.Message; }
    }

    private async Task ResizePage()
    {
        if (!ready || Viewport.ActualWidth < 1 || Viewport.ActualHeight < 1) return;
        double scale = Root.XamlRoot?.RasterizationScale ?? 1;
        await Send(BrowserWire.Command("Resize", new { width = (uint)Math.Clamp(Viewport.ActualWidth * scale, 1, 4096), height = (uint)Math.Clamp(Viewport.ActualHeight * scale, 1, 4096) }), selectedTab);
    }
    private void ViewportChanged(object sender, SizeChangedEventArgs args) { resizeTimer.Stop(); resizeTimer.Start(); }
    private async void AddTab(TabView sender, object args) => await Send(BrowserWire.Command("OpenTab", new { url = (string?)null }));
    private async void CloseTab(TabView sender, TabViewTabCloseRequestedEventArgs args) => await Send("CloseTab", (ulong)args.Tab.Tag);
    private async void SelectTab(object sender, SelectionChangedEventArgs args)
    {
        if (changingTabs || Tabs.SelectedItem is not TabViewItem item) return;
        selectedTab = (ulong)item.Tag;
        ShowSelected();
        await Send("GetHistoryState", selectedTab);
        await ResizePage();
    }
    private async void NavigateAddress(object sender, RoutedEventArgs args)
    {
        string url = Address.Text.Trim();
        if (url.Length == 0 || selectedTab is null) return;
        if (!url.Contains(':')) url = "https://" + url;
        Status.Text = "Loading…";
        await Send(BrowserWire.Command("Navigate", new { url }), selectedTab);
    }
    private void AddressKeyDown(object sender, KeyRoutedEventArgs args) { if (args.Key == VirtualKey.Enter) { args.Handled = true; NavigateAddress(sender, args); } }
    private async void GoBack(object sender, RoutedEventArgs args) => await Send("GoBack", selectedTab);
    private async void GoForward(object sender, RoutedEventArgs args) => await Send("GoForward", selectedTab);
    private async void Reload(object sender, RoutedEventArgs args)
    {
        if (selectedTab is ulong tab && urls.TryGetValue(tab, out string? url) && url.Length != 0)
            await Send(BrowserWire.Command("Navigate", new { url }), tab);
    }
    private async void OpenSettings(object sender, RoutedEventArgs args) => await Send(BrowserWire.Command("Navigate", new { url = "about:settings" }), selectedTab);
    private async void PageClick(object sender, PointerRoutedEventArgs args)
    {
        if (selectedTab is not ulong tab || !frames.TryGetValue(tab, out var frame)) return;
        var point = args.GetCurrentPoint(PageImage);
        if (!point.Properties.IsLeftButtonPressed) return;
        PageImage.Focus(FocusState.Pointer);
        await Send(BrowserWire.Command("Click", new { x = point.Position.X * frame.Image.PixelWidth / Math.Max(1, PageImage.ActualWidth), y = point.Position.Y * frame.Image.PixelHeight / Math.Max(1, PageImage.ActualHeight) }), tab);
    }
    private async void PageScroll(object sender, PointerRoutedEventArgs args) { args.Handled = true; await Send(BrowserWire.Command("Scroll", new { delta_y = -args.GetCurrentPoint(PageImage).Properties.MouseWheelDelta / 3.0 }), selectedTab); }
    private async void PageCharacter(UIElement sender, CharacterReceivedRoutedEventArgs args) { if (args.Character >= 32) { args.Handled = true; await Send(BrowserWire.Command("InsertText", new { text = char.ConvertFromUtf32((int)args.Character) }), selectedTab); } }
    private async void PageKey(object sender, KeyRoutedEventArgs args) { if (args.Key == VirtualKey.Back) { args.Handled = true; await Send("DeleteBackward", selectedTab); } }
}
