import { screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { ClearConfirmDialog } from "./ClearConfirmDialog";

describe("ClearConfirmDialog — confirmation dialog", () => {
  it("renders nothing when open is false", () => {
    renderWithProviders(
      <ClearConfirmDialog
        open={false}
        title="清空聊天记录"
        description="确定要清空吗？"
        onConfirm={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.queryByText("清空聊天记录")).toBeNull();
  });

  it("renders title, description, and '此操作不可撤销' warning when open", () => {
    renderWithProviders(
      <ClearConfirmDialog
        open={true}
        title="清空聊天记录"
        description="确定要清空全部 5 条聊天记录吗？"
        onConfirm={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByText("清空聊天记录")).toBeTruthy();
    expect(screen.getByText("确定要清空全部 5 条聊天记录吗？")).toBeTruthy();
    expect(screen.getByText("此操作不可撤销")).toBeTruthy();
  });

  it("calls onConfirm and onClose when confirm button is clicked", () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    renderWithProviders(
      <ClearConfirmDialog
        open={true}
        title="清空聊天记录"
        description="确定吗？"
        onConfirm={onConfirm}
        onClose={onClose}
      />,
    );
    fireEvent.click(screen.getByText("确认"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when cancel button is clicked", () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    renderWithProviders(
      <ClearConfirmDialog
        open={true}
        title="清空聊天记录"
        description="确定吗？"
        onConfirm={onConfirm}
        onClose={onClose}
      />,
    );
    fireEvent.click(screen.getByText("取消"));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
