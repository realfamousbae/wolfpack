export class Index {
  private items: string[] = [];

  add(word: string): void {
    this.items.push(word);
  }

  len(): bigint {
    return BigInt(this.items.length);
  }

  has(word: string): boolean {
    return this.items.includes(word);
  }

  joined(sep: string): string {
    return this.items.join(sep);
  }

  #secret(): void {}
}

export function newIndex(): Index {
  return new Index();
}
