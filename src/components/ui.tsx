import {
  useRef,
  useState,
  type ComponentProps,
  type ReactElement,
  type ReactNode,
} from "react";
import {
  Button as RadixButton,
  AlertDialog,
  Badge,
  Box,
  Card as RadixCard,
  Flex,
  Strong,
  Text,
  Checkbox as RadixCheckbox,
  Dialog,
  Select as RadixSelect,
  SegmentedControl as RadixSegmentedControl,
  Switch as RadixSwitch,
  TextArea as RadixTextArea,
  TextField as RadixTextField,
  Tooltip as RadixTooltip,
} from "@radix-ui/themes";
import { Check, Copy, Inbox, X } from "lucide-react";

/** Shared controls keep desktop pages on the same visual and interaction contract. */
let lastButtonFocus: HTMLButtonElement | null = null;

export { Badge };

export function Tooltip({
  content,
  children,
  ...props
}: Omit<ComponentProps<typeof RadixTooltip>, "children"> & {
  children: ReactElement;
}) {
  return content ? (
    <RadixTooltip content={content} {...props}>
      {children}
    </RadixTooltip>
  ) : (
    children
  );
}

export function Card({
  className = "",
  title,
  children,
  ...props
}: ComponentProps<"article">) {
  const card = (
    <RadixCard asChild className={`ui-card ${className}`}>
      <article {...props}>{children}</article>
    </RadixCard>
  );
  return <Tooltip content={title}>{card}</Tooltip>;
}

export function Button({
  className = "",
  type = "button",
  children,
  onFocus,
  title,
  ...props
}: Omit<ComponentProps<"button">, "color">) {
  const isClose = className.split(" ").includes("close");
  const control = (
    <RadixButton
      type={type}
      size="2"
      variant={className.split(" ").includes("primary") ? "solid" : "surface"}
      className={`ui-button ${className}`}
      aria-label={isClose ? "关闭" : undefined}
      onFocus={(event) => {
        lastButtonFocus = event.currentTarget;
        onFocus?.(event);
      }}
      {...props}
    >
      {isClose && children === "×" ? (
        <X size={17} aria-hidden="true" />
      ) : (
        children
      )}
    </RadixButton>
  );
  return title ? <Tooltip content={title}>{control}</Tooltip> : control;
}

export function TextField({
  className = "",
  ...props
}: ComponentProps<typeof RadixTextField.Root>) {
  return (
    <RadixTextField.Root
      size="2"
      className={`ui-text-field ${className}`}
      {...props}
    />
  );
}

export function TextArea({
  className = "",
  ...props
}: ComponentProps<typeof RadixTextArea>) {
  return (
    <RadixTextArea
      size="2"
      className={`ui-text-area ${className}`}
      {...props}
    />
  );
}

// Encode every value so empty filters and numeric values round-trip through Radix.
const selectValue = (value: string | number) => `value:${value}`;

export function Select({
  value,
  onValueChange,
  children,
  disabled,
  className = "",
  ...props
}: Omit<
  ComponentProps<typeof RadixSelect.Trigger>,
  "children" | "value" | "onChange"
> & {
  value: string | number;
  onValueChange: (value: string) => void;
  children: ReactNode;
}) {
  return (
    <RadixSelect.Root
      size="2"
      value={selectValue(value)}
      onValueChange={(next) => onValueChange(next.slice(6))}
      disabled={disabled}
    >
      <RadixSelect.Trigger className={`ui-select ${className}`} {...props} />
      <RadixSelect.Content
        position="popper"
        sideOffset={4}
        className="ui-select-content"
      >
        {children}
      </RadixSelect.Content>
    </RadixSelect.Root>
  );
}

export function SelectOption({
  value,
  ...props
}: Omit<ComponentProps<typeof RadixSelect.Item>, "value"> & {
  value: string | number;
}) {
  return <RadixSelect.Item value={selectValue(value)} {...props} />;
}

export function Checkbox({
  onCheckedChange,
  className = "",
  ...props
}: Omit<ComponentProps<typeof RadixCheckbox>, "onCheckedChange"> & {
  onCheckedChange?: (checked: boolean) => void;
}) {
  return (
    <RadixCheckbox
      className={`ui-checkbox ${className}`}
      onCheckedChange={(checked) => onCheckedChange?.(checked === true)}
      {...props}
    />
  );
}

export function Toggle({
  checked,
  onChange,
  disabled,
  label = "切换设置",
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <RadixSwitch
      aria-label={label}
      checked={checked}
      className="switch"
      disabled={disabled}
      onCheckedChange={onChange}
    />
  );
}

export function SegmentedControl<T extends string>({
  value,
  onChange,
  options,
  label,
  className = "",
  disabled,
}: {
  value: T;
  onChange: (value: T) => void;
  options: ReadonlyArray<{ value: T; label: ReactNode; tooltip?: string }>;
  label: string;
  className?: string;
  disabled?: boolean;
}) {
  return (
    <RadixSegmentedControl.Root
      size="2"
      aria-label={label}
      className={`segmented-control ${className}`}
      disabled={disabled}
      value={value}
      onValueChange={(next) => {
        const option = options.find((item) => item.value === next);
        if (option) onChange(option.value);
      }}
    >
      {options.map((option) => {
        const item = (
          <RadixSegmentedControl.Item
            key={option.value}
            className={value === option.value ? "active" : ""}
            aria-label={option.tooltip}
            value={option.value}
          >
            {option.label}
          </RadixSegmentedControl.Item>
        );
        return option.tooltip ? (
          <Tooltip key={option.value} content={option.tooltip}>
            {item}
          </Tooltip>
        ) : (
          item
        );
      })}
    </RadixSegmentedControl.Root>
  );
}

export function DialogSurface({
  children,
  title,
  onClose,
  className = "",
  drawer = false,
  closeDisabled = false,
  dismissOnOutside = true,
}: {
  children: ReactNode;
  title: string;
  onClose: () => void;
  className?: string;
  drawer?: boolean;
  closeDisabled?: boolean;
  dismissOnOutside?: boolean;
}) {
  // Async openers may briefly disable the focused button before the dialog mounts.
  // Preserve that opener as well as normal row/button focus without a Radix Trigger.
  const returnFocus = useRef(
    document.activeElement instanceof HTMLElement &&
      document.activeElement !== document.body
      ? document.activeElement
      : lastButtonFocus,
  );
  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open && !closeDisabled) onClose();
      }}
    >
      <Dialog.Content
        aria-describedby={undefined}
        maxWidth="none"
        className={`${drawer ? "provider-drawer" : "latency-dialog"} ${className}`}
        onCloseAutoFocus={(event) => {
          if (returnFocus.current?.isConnected) {
            event.preventDefault();
            returnFocus.current.focus();
          }
        }}
        onEscapeKeyDown={(event) => {
          if (closeDisabled) event.preventDefault();
        }}
        onInteractOutside={(event) => {
          if (closeDisabled || !dismissOnOutside) event.preventDefault();
        }}
      >
        <Dialog.Title className="sr-only">{title}</Dialog.Title>
        {children}
      </Dialog.Content>
    </Dialog.Root>
  );
}

export function ConfirmDialog({
  title,
  children,
  busy,
  error,
  onClose,
  onConfirm,
}: {
  title: string;
  children: ReactNode;
  busy: boolean;
  error?: string;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const returnFocus = useRef(lastButtonFocus);
  return (
    <AlertDialog.Root
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
    >
      <AlertDialog.Content
        className="latency-dialog confirmation-dialog"
        maxWidth="none"
        onEscapeKeyDown={(event) => {
          if (busy) event.preventDefault();
        }}
        onCloseAutoFocus={(event) => {
          if (returnFocus.current?.isConnected) {
            event.preventDefault();
            returnFocus.current.focus();
          }
        }}
      >
        <header>
          <AlertDialog.Title>{title}</AlertDialog.Title>
        </header>
        <AlertDialog.Description className="unsaved-route-dialog-body">
          {children}
          {error && (
            <Text
              as="span"
              color="red"
              role="alert"
              style={{ display: "block", marginTop: 8 }}
            >
              {error}
            </Text>
          )}
        </AlertDialog.Description>
        <footer>
          <AlertDialog.Cancel>
            <Button className="ghost" disabled={busy}>
              取消
            </Button>
          </AlertDialog.Cancel>
          <Button
            className="danger"
            disabled={busy}
            onClick={() => void onConfirm()}
          >
            {busy ? "正在删除…" : "确认删除"}
          </Button>
        </footer>
      </AlertDialog.Content>
    </AlertDialog.Root>
  );
}

export function EmptyState({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <Flex className="empty-state" direction="column" align="center">
      <Box className="empty-state-icon">
        <Inbox size={26} strokeWidth={1.5} aria-hidden="true" />
      </Box>
      <Strong>{title}</Strong>
      {description && <Text as="p">{description}</Text>}
      {action}
    </Flex>
  );
}

export function CopyButton({
  text,
  label = "复制",
}: {
  text: string;
  label?: string;
}) {
  const [result, setResult] = useState<"idle" | "copied" | "failed">("idle");
  return (
    <Button
      aria-label={
        result === "copied"
          ? `${label}，已复制`
          : result === "failed"
            ? `${label}，复制失败`
            : label
      }
      className="ghost small"
      title={result === "failed" ? "复制失败，请手动选择文本" : label}
      onBlur={() => setResult("idle")}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setResult("copied");
        } catch {
          setResult("failed");
        }
      }}
    >
      {result === "copied" ? (
        <Check size={13} aria-hidden="true" />
      ) : (
        <Copy size={13} aria-hidden="true" />
      )}
      <span>
        {result === "copied"
          ? "已复制"
          : result === "failed"
            ? "复制失败"
            : "复制"}
      </span>
    </Button>
  );
}
