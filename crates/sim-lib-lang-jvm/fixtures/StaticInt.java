public final class StaticInt {
    private StaticInt() {}

    public static int wholePipeline(int left, int right) {
        int sum = left + right;
        return sum * 2;
    }
}
