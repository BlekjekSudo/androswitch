package org.androswitch.fixture

class KotlinBridge {
    external fun nativeValue(value: Int): Int
    fun callback(value: Int): Int = value * 5 + 9
}
fun exercise(): Int {
    System.loadLibrary("kotlinbridge")
    return KotlinBridge().nativeValue(7)
}
fun main() { println(exercise()) }
