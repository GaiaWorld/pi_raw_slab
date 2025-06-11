# 自动扩展内存板实现

本库提供了两种自动扩展的内存板实现，针对不同环境优化：

## 结构体

### `VBSlab` (基于桶的内存板)
- **设计目标**：多线程环境使用
- **核心机制**：主内存板(可扩展Vec) + 辅助内存板(固定大小桶)
- **扩展方式**：主内存板满时，线程安全地在辅助桶分配
- **内存整理**：支持通过`settle()`合并所有数据到主内存板
- **线程安全**：需外部保证不会同时访问同一元素

### `VecSlab` (基于Vec的内存板)
- **设计目标**：WASM环境专用
- **核心机制**：单个自动扩展的Vec
- **特点**：实现简单，无桶结构
- **线程安全**：非线程安全实现

## 主要方法

### 通用方法
- `with_capacity(raw_size: usize, capacity: usize) -> Self`  
  创建指定元素大小和初始容量的内存板
  
- `capacity(len: usize) -> usize`  
  计算给定元素数量所需的总容量
  
- `vec_capacity() -> usize`  
  获取主内存板的容量(元素数量)
  
- `get<T>(index: usize) -> Option<&mut T>`  
  安全获取元素引用(边界检查)
  
- `get_unchecked<T>(index: usize) -> &mut T`  
  无检查获取元素引用(需确保索引有效)
  
- `load_alloc<T>(index: usize) -> &mut T`  
  获取元素引用(必要时自动分配)
  
- `settle(len: usize)`  
  内存整理(实现机制不同)

## 使用选择
- **多线程环境**：使用 `VBSlab`
- **WASM环境**：使用 `VecSlab`
- **类型别名**：
  - 启用 "rc" 特性时：`RawSlab` = `VecSlab`
  - 未启用时：`RawSlab` = `VBSlab`

## 注意事项
1. 创建时必须指定元素大小(`raw_size`)
2. 零大小元素会特殊处理
3. 多线程访问需外部同步机制
4. 定期调用`settle()`可优化内存布局