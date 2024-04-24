// my-component.go
package main

import (
	"fmt"

	api "example.com/api"
)

//go:generate wit-bindgen tiny-go ../../wit --out-dir=api
func main() {
	devices := api.StaticUsbDeviceEnumerate()
	for _, device := range devices {
		descriptor := device.Descriptor()
		vendorId := descriptor.VendorId
		productId := descriptor.ProductId
		productName := "N/A"
		if descriptor.ProductName.IsSome() {
			productName = descriptor.ProductName.Unwrap()
		}
		manufacturerName := "N/A"
		if descriptor.ManufacturerName.IsSome() {
			manufacturerName = descriptor.ManufacturerName.Unwrap()
		}
		fmt.Printf("%04x:%04x - %s %s\n", vendorId, productId, manufacturerName, productName)
	}
}
