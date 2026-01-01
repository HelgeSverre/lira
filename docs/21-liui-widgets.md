# Lira Lira UI Widgets Specification

## Document Information

| Property          | Value               |
| ----------------- | ------------------- |
| **Document ID**   | 21-liui-widgets     |
| **Version**       | 1.0.0-draft         |
| **Status**        | Draft Specification |
| **Prerequisites** | 20-liui-format      |

---

## Table of Contents

1. [Overview](#1-overview)
2. [Window](#2-window)
3. [Layout Containers](#3-layout-containers)
4. [Text Widgets](#4-text-widgets)
5. [Button Widgets](#5-button-widgets)
6. [Input Widgets](#6-input-widgets)
7. [Selection Widgets](#7-selection-widgets)
8. [Display Widgets](#8-display-widgets)
9. [Navigation Widgets](#9-navigation-widgets)
10. [Overlay Widgets](#10-overlay-widgets)
11. [Custom Drawing](#11-custom-drawing)
12. [Common Properties](#12-common-properties)

---

## 1. Overview

### 1.1 Widget Categories

```
┌─────────────────────────────────────────────────────────────────┐
│                    Lira UI WIDGET HIERARCHY                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Window                                                         │
│  ├── Layout Containers                                          │
│  │   ├── VBox, HBox, ZStack                                    │
│  │   ├── Grid, Flex                                            │
│  │   ├── ScrollView                                            │
│  │   └── Spacer, Divider                                       │
│  ├── Text Widgets                                               │
│  │   ├── Label, Text, RichText                                 │
│  │   └── Link                                                  │
│  ├── Button Widgets                                             │
│  │   ├── Button, IconButton                                    │
│  │   └── ToggleButton, FloatingActionButton                    │
│  ├── Input Widgets                                              │
│  │   ├── TextField, TextArea                                   │
│  │   ├── NumberField, SearchField                              │
│  │   └── PasswordField                                         │
│  ├── Selection Widgets                                          │
│  │   ├── Checkbox, Radio, Switch                               │
│  │   ├── Slider, RangeSlider                                   │
│  │   └── Select, DatePicker, ColorPicker                       │
│  ├── Display Widgets                                            │
│  │   ├── Image, Icon, Avatar                                   │
│  │   ├── ProgressBar, Spinner                                  │
│  │   └── Badge, Chip, Tag                                      │
│  ├── Navigation                                                 │
│  │   ├── TabView, TabBar                                       │
│  │   ├── NavigationView, Sidebar                               │
│  │   └── Breadcrumb, Pagination                                │
│  └── Overlays                                                   │
│      ├── Dialog, Modal                                         │
│      ├── Tooltip, Popover                                      │
│      └── Toast, Snackbar                                       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 Import

All built-in widgets are available without import:

```liui
// Direct usage
VBox {
    Label { text: "Hello" }
    Button { text: "Click" }
}

// Explicit import (optional)
import { VBox, Label, Button } from "liui:widgets"
```

---

## 2. Window

### 2.1 Window

Top-level application window.

```liui
Window {
    // Required
    title: "My Application"

    // Size
    width: 800
    height: 600
    minWidth: 400
    minHeight: 300
    maxWidth: 1920
    maxHeight: 1080

    // Position
    x: 100
    y: 100
    centered: true          // Center on screen

    // Appearance
    resizable: true
    closable: true
    minimizable: true
    maximizable: true
    fullscreen: false
    alwaysOnTop: false

    // Frame
    decorated: true         // Show title bar
    transparent: false

    // Events
    onClose: () => {}
    onResize: (width, height) => {}
    onMove: (x, y) => {}
    onFocus: () => {}
    onBlur: () => {}

    // Content
    VBox {
        // Window content
    }
}
```

### 2.2 Dialog Window

Modal dialog window.

```liui
Dialog {
    title: "Confirm"
    open: ${state.showDialog}
    width: 400
    height: 200

    modal: true             // Block interaction with parent
    closeOnEscape: true
    closeOnClickOutside: false

    onClose: () => state.showDialog = false

    VBox {
        Label { text: "Are you sure?" }

        HBox {
            Button { text: "Cancel", onClick: () => state.showDialog = false }
            Button { text: "Confirm", primary: true, onClick: confirm }
        }
    }
}
```

---

## 3. Layout Containers

### 3.1 VBox (Vertical Box)

Stack children vertically.

```liui
VBox {
    // Spacing between children
    spacing: 16

    // Padding inside container
    padding: 24
    // Or individual sides
    paddingTop: 16
    paddingBottom: 16
    paddingLeft: 24
    paddingRight: 24

    // Alignment
    align: "start"          // start, center, end, stretch
    justify: "start"        // start, center, end, space-between, space-around, space-evenly

    // Fill available space
    fill: true

    // Children
    Label { text: "First" }
    Label { text: "Second" }
    Label { text: "Third" }
}
```

### 3.2 HBox (Horizontal Box)

Stack children horizontally.

```liui
HBox {
    spacing: 8
    padding: 16
    align: "center"
    justify: "space-between"

    Label { text: "Left" }
    Spacer { }              // Flexible space
    Label { text: "Right" }
}
```

### 3.3 ZStack (Z-Axis Stack)

Overlay children on top of each other.

```liui
ZStack {
    alignment: "center"     // Where children align

    Image { src: "background.png", fill: true }

    VBox {
        // Content overlaid on image
        Label { text: "Overlay Text" }
    }
}
```

### 3.4 Grid

Grid layout with rows and columns.

```liui
Grid {
    columns: 3              // Number of columns
    rows: 2                 // Number of rows (optional)
    columnGap: 16
    rowGap: 16
    padding: 24

    // Auto-sizing columns
    columnSizes: ["1fr", "2fr", "1fr"]  // Fractional units
    // Or fixed sizes
    columnSizes: [100, "auto", 200]

    // Children placed in order
    GridItem { Label { text: "1" } }
    GridItem { Label { text: "2" } }
    GridItem { Label { text: "3" } }
    GridItem { Label { text: "4" } }
    GridItem { Label { text: "5" } }
    GridItem { Label { text: "6" } }

    // Span multiple cells
    GridItem {
        column: 1           // Start column (0-indexed)
        columnSpan: 2       // Span 2 columns
        row: 0
        rowSpan: 1

        Label { text: "Spans 2 columns" }
    }
}
```

### 3.5 Flex

Flexible box layout.

```liui
Flex {
    direction: "row"        // row, column, row-reverse, column-reverse
    wrap: "wrap"            // nowrap, wrap, wrap-reverse
    gap: 16
    alignItems: "center"    // start, center, end, stretch, baseline
    justifyContent: "start" // start, center, end, space-between, etc.
    alignContent: "start"   // For multi-line flex containers

    FlexItem {
        flex: 1             // Grow factor
        flexShrink: 0       // Shrink factor
        flexBasis: "auto"   // Initial size
        alignSelf: "center" // Override alignItems

        Label { text: "Flexible" }
    }

    FlexItem {
        flex: 2             // Takes 2x space

        Label { text: "More Flexible" }
    }
}
```

### 3.6 ScrollView

Scrollable container.

```liui
ScrollView {
    // Scroll direction
    direction: "vertical"   // vertical, horizontal, both

    // Scrollbar visibility
    showScrollbar: "auto"   // auto, always, never

    // Scroll behavior
    bounces: true           // Elastic bounce at edges
    paging: false           // Snap to pages

    // Events
    onScroll: (x, y) => {}
    onScrollEnd: () => {}

    // Scroll position (two-way bindable)
    scrollX: <=> state.scrollX
    scrollY: <=> state.scrollY

    // Content
    VBox {
        // Tall content...
    }
}
```

### 3.7 Spacer

Flexible empty space.

```liui
HBox {
    Label { text: "Left" }
    Spacer { }              // Takes all available space
    Label { text: "Right" }
}

// Fixed size spacer
VBox {
    Label { text: "Top" }
    Spacer { size: 32 }     // Fixed 32px space
    Label { text: "Bottom" }
}
```

### 3.8 Divider

Visual separator line.

```liui
VBox {
    Label { text: "Section 1" }

    Divider {
        orientation: "horizontal"  // horizontal, vertical
        color: "#ddd"
        thickness: 1
        margin: 16
    }

    Label { text: "Section 2" }
}
```

---

## 4. Text Widgets

### 4.1 Label

Single-line text display.

```liui
Label {
    text: "Hello, World!"

    // Typography
    fontSize: 16
    fontWeight: "bold"      // normal, bold, 100-900
    fontFamily: "system-ui"
    fontStyle: "normal"     // normal, italic

    // Appearance
    color: "#333"
    backgroundColor: "transparent"

    // Text handling
    selectable: false       // Allow text selection
    truncate: "end"         // none, start, middle, end
    maxLines: 1

    // Accessibility
    accessibilityLabel: "Greeting text"
}
```

### 4.2 Text

Multi-line text display.

```liui
Text {
    content: """
        This is a longer text that can
        span multiple lines and will wrap
        automatically.
    """

    // Line handling
    lineHeight: 1.5
    textAlign: "left"       // left, center, right, justify
    maxLines: 3
    overflow: "ellipsis"    // visible, hidden, ellipsis

    // Paragraph
    paragraphSpacing: 16
    firstLineIndent: 24
}
```

### 4.3 RichText

Text with inline formatting.

```liui
RichText {
    // Markdown-like syntax
    content: """
        This is **bold** and this is *italic*.
        This is a [link](https://example.com).
        Code: `inline code`.
    """

    // Or structured spans
    spans: [
        { text: "Normal " }
        { text: "Bold", style: { fontWeight: "bold" } }
        { text: " and " }
        { text: "Colored", style: { color: "#007AFF" } }
    ]
}
```

### 4.4 Link

Clickable text link.

```liui
Link {
    text: "Click here"
    href: "https://example.com"

    // Or custom action
    onClick: () => navigate("/page")

    // Appearance
    color: "#007AFF"
    underline: true
    hoverColor: "#0056b3"
}
```

---

## 5. Button Widgets

### 5.1 Button

Standard clickable button.

```liui
Button {
    text: "Click Me"

    // Variants
    variant: "primary"      // primary, secondary, outline, text, destructive

    // Size
    size: "medium"          // small, medium, large

    // State
    disabled: false
    loading: false

    // Icon
    icon: "plus"
    iconPosition: "start"   // start, end

    // Events
    onClick: () => doSomething()
    onLongPress: () => showMenu()

    // Style overrides
    style: {
        backgroundColor: "#007AFF"
        borderRadius: 8
    }
}
```

### 5.2 IconButton

Button with only an icon.

```liui
IconButton {
    icon: "menu"

    // Size
    size: 48

    // Appearance
    color: "#333"
    hoverColor: "#007AFF"
    backgroundColor: "transparent"

    // Tooltip
    tooltip: "Open menu"

    onClick: () => toggleMenu()
}
```

### 5.3 ToggleButton

Button that toggles between states.

```liui
ToggleButton {
    pressed: <=> state.isActive

    icon: ${state.isActive ? "star-filled" : "star-outline"}
    text: ${state.isActive ? "Favorited" : "Favorite"}

    onChange: (pressed) => {}
}
```

### 5.4 FloatingActionButton

Floating action button (FAB).

```liui
FloatingActionButton {
    icon: "plus"

    // Position
    position: "bottom-right"
    offsetX: 24
    offsetY: 24

    // Extended FAB with text
    extended: true
    text: "Add Item"

    // Size
    size: "regular"         // mini, regular, extended

    onClick: () => addItem()
}
```

---

## 6. Input Widgets

### 6.1 TextField

Single-line text input.

```liui
TextField {
    // Value binding
    value: <=> state.name

    // Placeholder
    placeholder: "Enter your name"

    // Label
    label: "Name"
    helperText: "Your full name"
    errorText: ${state.nameError}

    // Constraints
    maxLength: 100
    minLength: 1
    pattern: "[a-zA-Z ]+"

    // Appearance
    variant: "outlined"     // outlined, filled, underlined

    // Icons
    prefixIcon: "person"
    suffixIcon: "clear"
    onSuffixClick: () => state.name = ""

    // Events
    onInput: (value) => {}
    onFocus: () => {}
    onBlur: () => {}
    onSubmit: () => {}      // Enter pressed

    // State
    disabled: false
    readonly: false
    required: true
}
```

### 6.2 TextArea

Multi-line text input.

```liui
TextArea {
    value: <=> state.description

    placeholder: "Enter description..."
    label: "Description"

    // Size
    rows: 4
    minRows: 2
    maxRows: 10
    autoGrow: true          // Grow with content

    // Constraints
    maxLength: 1000
    showCharCount: true

    // Resize behavior
    resize: "vertical"      // none, horizontal, vertical, both
}
```

### 6.3 NumberField

Numeric input.

```liui
NumberField {
    value: <=> state.quantity

    label: "Quantity"

    // Constraints
    min: 0
    max: 100
    step: 1

    // Format
    decimalPlaces: 0
    prefix: ""
    suffix: " items"

    // Controls
    showSpinner: true       // Up/down buttons

    onChange: (value) => {}
}
```

### 6.4 SearchField

Search input with clear button.

```liui
SearchField {
    value: <=> state.searchQuery

    placeholder: "Search..."

    // Debounce search
    debounce: 300

    // Auto-complete
    suggestions: ${state.searchSuggestions}
    onSelectSuggestion: (suggestion) => {}

    // Events
    onSearch: (query) => performSearch(query)
    onClear: () => state.searchQuery = ""
}
```

### 6.5 PasswordField

Password input with visibility toggle.

```liui
PasswordField {
    value: <=> state.password

    label: "Password"
    placeholder: "Enter password"

    // Visibility toggle
    showToggle: true

    // Strength indicator
    showStrength: true
    strengthLevel: ${calculateStrength(state.password)}

    // Constraints
    minLength: 8
    pattern: ".*[A-Z].*"    // Require uppercase
}
```

---

## 7. Selection Widgets

### 7.1 Checkbox

Boolean toggle with checkmark.

```liui
Checkbox {
    checked: <=> state.agreedToTerms

    label: "I agree to the terms"

    // Indeterminate state
    indeterminate: false

    // Appearance
    color: "#007AFF"
    size: "medium"          // small, medium, large

    disabled: false

    onChange: (checked) => {}
}
```

### 7.2 Radio

Single selection from a group.

```liui
RadioGroup {
    value: <=> state.selectedOption

    label: "Choose an option"
    orientation: "vertical"  // horizontal, vertical

    options: [
        { value: "a", label: "Option A" }
        { value: "b", label: "Option B" }
        { value: "c", label: "Option C", disabled: true }
    ]

    onChange: (value) => {}
}

// Or individual Radio buttons
VBox {
    Radio {
        value: "a"
        selected: ${state.option == "a"}
        label: "Option A"
        onSelect: () => state.option = "a"
    }
    Radio {
        value: "b"
        selected: ${state.option == "b"}
        label: "Option B"
        onSelect: () => state.option = "b"
    }
}
```

### 7.3 Switch

Boolean toggle switch.

```liui
Switch {
    on: <=> state.darkMode

    label: "Dark Mode"
    labelPosition: "start"  // start, end

    // Appearance
    color: "#4CAF50"
    size: "medium"

    onChange: (on) => {}
}
```

### 7.4 Slider

Range value selector.

```liui
Slider {
    value: <=> state.volume

    // Range
    min: 0
    max: 100
    step: 1

    // Appearance
    showValue: true
    showTicks: true
    tickInterval: 10

    // Labels
    label: "Volume"
    minLabel: "0%"
    maxLabel: "100%"

    // Events
    onChange: (value) => {}
    onDragStart: () => {}
    onDragEnd: () => {}
}
```

### 7.5 RangeSlider

Dual-handle range selector.

```liui
RangeSlider {
    minValue: <=> state.priceMin
    maxValue: <=> state.priceMax

    min: 0
    max: 1000
    step: 10

    // Format
    formatValue: (v) => "$" + v

    label: "Price Range"

    onChange: (min, max) => {}
}
```

### 7.6 Select

Dropdown selection.

```liui
Select {
    value: <=> state.country

    label: "Country"
    placeholder: "Select a country"

    options: [
        { value: "us", label: "United States" }
        { value: "uk", label: "United Kingdom" }
        { value: "de", label: "Germany" }
    ]

    // Searchable
    searchable: true

    // Multiple selection
    multiple: false

    // Appearance
    variant: "outlined"

    onChange: (value) => {}
}

// Multiple selection
Select {
    value: <=> state.selectedTags
    multiple: true

    options: ${state.availableTags}

    // Display as chips
    renderSelected: "chips"

    maxSelections: 5
}
```

### 7.7 DatePicker

Date selection.

```liui
DatePicker {
    value: <=> state.birthDate

    label: "Birth Date"

    // Range constraints
    minDate: "1900-01-01"
    maxDate: ${today()}

    // Format
    format: "YYYY-MM-DD"
    displayFormat: "MMMM D, YYYY"

    // Picker type
    type: "date"            // date, month, year

    onChange: (date) => {}
}
```

### 7.8 ColorPicker

Color selection.

```liui
ColorPicker {
    value: <=> state.themeColor

    label: "Theme Color"

    // Presets
    presets: ["#FF0000", "#00FF00", "#0000FF"]

    // Allow alpha
    showAlpha: true

    // Format
    format: "hex"           // hex, rgb, hsl

    onChange: (color) => {}
}
```

---

## 8. Display Widgets

### 8.1 Image

Display an image.

```liui
Image {
    src: "image.png"

    // Or from URL
    src: "https://example.com/image.jpg"

    // Size
    width: 200
    height: 150

    // Scaling
    fit: "cover"            // contain, cover, fill, none, scale-down

    // Placeholder
    placeholder: "loading.gif"
    fallback: "error.png"

    // Alt text
    alt: "Description of image"

    // Loading
    loading: "lazy"         // eager, lazy

    // Events
    onLoad: () => {}
    onError: () => {}
}
```

### 8.2 Icon

Display an icon from icon set.

```liui
Icon {
    name: "home"

    // Size
    size: 24

    // Color
    color: "#333"

    // Icon set
    set: "material"         // material, feather, heroicons

    // Accessibility
    accessibilityLabel: "Home"
}
```

### 8.3 Avatar

User avatar display.

```liui
Avatar {
    // Image source
    src: ${user.avatarUrl}

    // Fallback to initials
    name: ${user.name}

    // Size
    size: 48              // or "small", "medium", "large"

    // Shape
    shape: "circle"       // circle, square, rounded

    // Status indicator
    status: "online"      // online, offline, away, busy

    // Placeholder
    placeholder: "user"   // Icon name
}

// Avatar group
AvatarGroup {
    max: 4
    size: 32

    @for (user in state.users) {
        Avatar { src: ${user.avatar}, name: ${user.name} }
    }
}
```

### 8.4 ProgressBar

Progress indicator.

```liui
ProgressBar {
    value: ${state.progress}

    // Range
    min: 0
    max: 100

    // Appearance
    color: "#4CAF50"
    backgroundColor: "#E0E0E0"
    height: 8
    borderRadius: 4

    // Indeterminate (loading)
    indeterminate: false

    // Label
    showLabel: true
    labelPosition: "right"  // left, right, center, inside
    formatLabel: (v) => v + "%"
}
```

### 8.5 Spinner

Loading spinner.

```liui
Spinner {
    size: 32

    // Style
    variant: "circular"     // circular, dots, bars

    // Color
    color: "#007AFF"

    // Speed
    duration: 1000          // ms per rotation
}
```

### 8.6 Badge

Notification badge.

```liui
Badge {
    content: ${state.unreadCount}

    // Position relative to child
    position: "top-right"

    // Appearance
    color: "#FF3B30"
    textColor: "#FFFFFF"

    // Show/hide
    visible: ${state.unreadCount > 0}

    // Max value
    max: 99

    // Dot mode (no number)
    dot: false

    // Child element
    Icon { name: "notifications" }
}
```

### 8.7 Chip

Compact element for input or display.

```liui
Chip {
    label: "Technology"

    // Variants
    variant: "filled"       // filled, outlined

    // Deletable
    deletable: true
    onDelete: () => removeTag("technology")

    // Selectable
    selected: ${state.selectedTags.includes("technology")}
    onClick: () => toggleTag("technology")

    // Icon
    icon: "tag"
    avatar: ${user.avatar}
}

// Chip group
ChipGroup {
    @for (tag in state.tags) {
        Chip {
            key: ${tag.id}
            label: ${tag.name}
            deletable: true
            onDelete: () => removeTag(tag.id)
        }
    }
}
```

---

## 9. Navigation Widgets

### 9.1 TabView

Tabbed content container.

```liui
TabView {
    selectedIndex: <=> state.activeTab

    // Tab definitions
    tabs: [
        { label: "Home", icon: "home" }
        { label: "Settings", icon: "settings" }
        { label: "Profile", icon: "person" }
    ]

    // Tab bar position
    position: "top"         // top, bottom

    // Tab content
    @tab (index: 0) {
        HomeScreen { }
    }
    @tab (index: 1) {
        SettingsScreen { }
    }
    @tab (index: 2) {
        ProfileScreen { }
    }
}
```

### 9.2 Sidebar

Collapsible sidebar navigation.

```liui
Sidebar {
    expanded: <=> state.sidebarExpanded

    width: 250
    collapsedWidth: 64

    // Header
    header: {
        Logo { }
    }

    // Navigation items
    SidebarItem {
        icon: "home"
        label: "Home"
        active: ${state.route == "/"}
        onClick: () => navigate("/")
    }
    SidebarItem {
        icon: "folder"
        label: "Files"
        active: ${state.route.startsWith("/files")}
        onClick: () => navigate("/files")
    }

    SidebarDivider { }

    SidebarSection {
        title: "Settings"

        SidebarItem { icon: "person", label: "Profile" }
        SidebarItem { icon: "settings", label: "Preferences" }
    }

    // Footer
    footer: {
        UserMenu { user: ${state.currentUser} }
    }
}
```

### 9.3 Breadcrumb

Navigation breadcrumb trail.

```liui
Breadcrumb {
    separator: "/"          // Or icon: "chevron-right"

    items: [
        { label: "Home", href: "/" }
        { label: "Products", href: "/products" }
        { label: "Electronics", href: "/products/electronics" }
        { label: "Laptops" }  // Current page (no href)
    ]

    // Collapse when too many
    maxItems: 4
    collapseAt: "start"     // start, middle
}
```

### 9.4 Pagination

Page navigation.

```liui
Pagination {
    currentPage: <=> state.page
    totalPages: ${Math.ceil(state.totalItems / state.pageSize)}

    // Display options
    showFirstLast: true     // First/last page buttons
    showPrevNext: true      // Prev/next buttons
    siblingCount: 1         // Pages shown around current

    // Size selector
    showPageSize: true
    pageSize: <=> state.pageSize
    pageSizeOptions: [10, 25, 50, 100]

    onChange: (page) => loadPage(page)
}
```

---

## 10. Overlay Widgets

### 10.1 Modal

Modal overlay.

```liui
Modal {
    open: <=> state.showModal

    // Behavior
    closeOnClickOutside: true
    closeOnEscape: true

    // Animation
    animation: "fade"       // fade, slide, zoom

    // Overlay
    overlayColor: "rgba(0, 0, 0, 0.5)"

    // Content
    ModalContent {
        width: 500
        maxHeight: "80vh"

        ModalHeader {
            title: "Modal Title"
            closeButton: true
        }

        ModalBody {
            padding: 24

            // Modal content
        }

        ModalFooter {
            Button { text: "Cancel", onClick: () => state.showModal = false }
            Button { text: "Save", primary: true, onClick: save }
        }
    }
}
```

### 10.2 Tooltip

Hover tooltip.

```liui
Tooltip {
    content: "This is a helpful tooltip"

    // Position
    position: "top"         // top, bottom, left, right

    // Delay
    showDelay: 500          // ms
    hideDelay: 100

    // Trigger
    trigger: "hover"        // hover, click, focus

    // Wrapped element
    Button { text: "Hover me" }
}

// Rich tooltip
Tooltip {
    position: "bottom"

    content: {
        VBox {
            Label { text: "Title", fontWeight: "bold" }
            Label { text: "Description text" }
        }
    }

    IconButton { icon: "info" }
}
```

### 10.3 Popover

Rich popup content.

```liui
Popover {
    open: <=> state.showPopover

    // Position
    position: "bottom-start"
    offset: 8

    // Content
    content: {
        VBox {
            padding: 16
            spacing: 8

            Label { text: "Actions" }
            Button { text: "Edit", onClick: edit }
            Button { text: "Delete", variant: "destructive", onClick: delete }
        }
    }

    // Trigger element
    Button { text: "More", onClick: () => state.showPopover = true }
}
```

### 10.4 Toast/Snackbar

Temporary notification message.

```liui
// Show toast programmatically
Button {
    text: "Show Toast"
    onClick: () => {
        toast({
            message: "Item saved successfully"
            type: "success"      // success, error, warning, info
            duration: 3000
            action: {
                label: "Undo"
                onClick: () => undo()
            }
        })
    }
}

// Toast container (place once in app)
ToastContainer {
    position: "bottom-right"
    maxToasts: 3
}
```

### 10.5 ContextMenu

Right-click context menu.

```liui
ContextMenu {
    // Target element
    ListItem {
        text: ${item.name}
    }

    // Menu items
    MenuItem { label: "Edit", icon: "edit", onClick: () => edit(item) }
    MenuItem { label: "Copy", icon: "copy", onClick: () => copy(item) }
    MenuDivider { }
    MenuItem { label: "Delete", icon: "delete", variant: "destructive", onClick: () => delete(item) }
}
```

---

## 11. Custom Drawing

### 11.1 Canvas

Custom 2D drawing surface.

```liui
Canvas {
    width: 400
    height: 300

    // Drawing function called on render
    onDraw: (ctx) => {
        // Clear
        ctx.clearRect(0, 0, 400, 300)

        // Draw rectangle
        ctx.fillStyle = "#007AFF"
        ctx.fillRect(50, 50, 100, 80)

        // Draw circle
        ctx.beginPath()
        ctx.arc(250, 100, 50, 0, Math.PI * 2)
        ctx.fill()

        // Draw text
        ctx.fillStyle = "#333"
        ctx.font = "16px system-ui"
        ctx.fillText("Hello Canvas", 150, 200)
    }

    // Redraw when these change
    dependencies: [state.data]

    // Events
    onMouseDown: (x, y) => {}
    onMouseMove: (x, y) => {}
    onMouseUp: (x, y) => {}
}
```

### 11.2 SVG

Inline SVG graphics.

```liui
SVG {
    width: 100
    height: 100
    viewBox: "0 0 100 100"

    // SVG elements
    Circle {
        cx: 50
        cy: 50
        r: 40
        fill: "#007AFF"
        stroke: "#0056b3"
        strokeWidth: 2
    }

    Rect {
        x: 10
        y: 10
        width: 30
        height: 30
        fill: "#4CAF50"
        rx: 4  // border radius
    }

    Path {
        d: "M10 80 Q 50 10, 90 80"
        stroke: "#333"
        strokeWidth: 2
        fill: "none"
    }

    Text {
        x: 50
        y: 55
        textAnchor: "middle"
        fill: "white"
        content: "SVG"
    }
}
```

---

## 12. Common Properties

### 12.1 Universal Properties

Available on all widgets:

```liui
AnyWidget {
    // Identification
    id: "myWidget"

    // Visibility
    visible: true
    opacity: 1.0

    // Layout
    width: 200
    height: 100
    minWidth: 50
    maxWidth: 500
    minHeight: 30
    maxHeight: 200

    // Margin (outside)
    margin: 8
    marginTop: 16
    marginBottom: 16
    marginLeft: 8
    marginRight: 8
    marginHorizontal: 8
    marginVertical: 16

    // Padding (inside, for containers)
    padding: 16
    paddingTop: 8
    paddingBottom: 8

    // Flex properties
    flex: 1
    flexGrow: 1
    flexShrink: 0
    alignSelf: "center"

    // Style object
    style: { }

    // Accessibility
    accessibilityLabel: "Description"
    accessibilityHint: "How to use"
    accessibilityRole: "button"

    // Pointer events
    pointerEvents: "auto"   // auto, none, box-only

    // Cursor
    cursor: "pointer"

    // Transform
    transform: "rotate(45deg)"

    // Events
    onClick: () => {}
    onDoubleClick: () => {}
    onMouseEnter: () => {}
    onMouseLeave: () => {}
    onFocus: () => {}
    onBlur: () => {}
}
```

### 12.2 Layout Property Values

```liui
// Size values
width: 100              // Pixels
width: "50%"            // Percentage of parent
width: "auto"           // Automatic
width: "fit-content"    // Fit to content

// Flex values
flex: 1                 // Grow factor
flex: "1 0 auto"        // grow shrink basis

// Alignment values
align: "start"          // start, center, end, stretch
justify: "space-between" // start, center, end, space-between, space-around, space-evenly
```

### 12.3 Color Values

```liui
color: "#007AFF"            // Hex
color: "#007AFF80"          // Hex with alpha
color: "rgb(0, 122, 255)"   // RGB
color: "rgba(0, 122, 255, 0.5)" // RGBA
color: "hsl(210, 100%, 50%)"    // HSL
color: "transparent"
color: "inherit"
color: ${theme.colors.primary} // Theme reference
```

---

## Appendix: Widget Quick Reference

| Widget         | Description        | Key Props                 |
| -------------- | ------------------ | ------------------------- |
| **Layout**     |                    |                           |
| VBox           | Vertical stack     | spacing, align, justify   |
| HBox           | Horizontal stack   | spacing, align, justify   |
| ZStack         | Overlay stack      | alignment                 |
| Grid           | Grid layout        | columns, rows, gap        |
| Flex           | Flexbox container  | direction, wrap, gap      |
| ScrollView     | Scrollable area    | direction, showScrollbar  |
| **Text**       |                    |                           |
| Label          | Single-line text   | text, fontSize, color     |
| Text           | Multi-line text    | content, textAlign        |
| Link           | Clickable link     | text, href, onClick       |
| **Buttons**    |                    |                           |
| Button         | Standard button    | text, variant, onClick    |
| IconButton     | Icon-only button   | icon, size, onClick       |
| **Inputs**     |                    |                           |
| TextField      | Text input         | value, label, placeholder |
| TextArea       | Multi-line input   | value, rows, autoGrow     |
| NumberField    | Numeric input      | value, min, max, step     |
| **Selection**  |                    |                           |
| Checkbox       | Boolean toggle     | checked, label            |
| Radio          | Single select      | value, options            |
| Switch         | Toggle switch      | on, label                 |
| Slider         | Range selector     | value, min, max           |
| Select         | Dropdown           | value, options            |
| **Display**    |                    |                           |
| Image          | Display image      | src, fit, alt             |
| Icon           | Display icon       | name, size, color         |
| ProgressBar    | Progress indicator | value, indeterminate      |
| Spinner        | Loading spinner    | size, color               |
| **Navigation** |                    |                           |
| TabView        | Tabbed content     | tabs, selectedIndex       |
| Sidebar        | Side navigation    | expanded, items           |
| **Overlays**   |                    |                           |
| Modal          | Modal dialog       | open, closeOnEscape       |
| Tooltip        | Hover tooltip      | content, position         |
| Toast          | Notification       | message, type, duration   |

---

_This document is part of the Lira Language Specification._
